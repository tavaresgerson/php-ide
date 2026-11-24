//! LSP snippet syntax (`insertTextFormat: 2`): `$1`, `${1:default}`,
//! `${1|one,two|}`, `$0`, variables and `\` escapes.
//!
//! [`expand`] turns a snippet into plain text plus its tab stops, as
//! character ranges into that text, in visiting order (`$0` last).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    pub text: String,
    /// `(start, end)` character ranges, in the order Tab visits them.
    pub stops: Vec<(usize, usize)>,
}

pub fn expand(snippet: &str) -> Expanded {
    let chars: Vec<char> = snippet.chars().collect();
    let mut out = String::new();
    let mut len = 0usize;
    let mut stops: Vec<(u32, usize, usize)> = Vec::new();
    parse(&chars, &mut 0, &mut out, &mut len, &mut stops, false);
    // Same index twice: the first occurrence is the stop.
    let mut seen = std::collections::HashSet::new();
    stops.retain(|(index, _, _)| seen.insert(*index));
    stops.sort_by_key(|(index, _, _)| if *index == 0 { u32::MAX } else { *index });
    Expanded {
        text: out,
        stops: stops.into_iter().map(|(_, s, e)| (s, e)).collect(),
    }
}

fn push(out: &mut String, len: &mut usize, c: char) {
    out.push(c);
    *len += 1;
}

/// Parses until the end, or until an unescaped `}` when `nested`.
fn parse(
    chars: &[char],
    i: &mut usize,
    out: &mut String,
    len: &mut usize,
    stops: &mut Vec<(u32, usize, usize)>,
    nested: bool,
) {
    while *i < chars.len() {
        let c = chars[*i];
        match c {
            '\\' if *i + 1 < chars.len()
                && matches!(chars[*i + 1], '$' | '}' | '\\' | ',' | '|') =>
            {
                push(out, len, chars[*i + 1]);
                *i += 2;
            }
            '}' if nested => return,
            '$' => {
                *i += 1;
                if *i < chars.len() && chars[*i].is_ascii_digit() {
                    let index = number(chars, i);
                    stops.push((index, *len, *len));
                } else if *i < chars.len() && chars[*i] == '{' {
                    *i += 1;
                    if *i < chars.len() && chars[*i].is_ascii_digit() {
                        let index = number(chars, i);
                        let start = *len;
                        match chars.get(*i) {
                            Some(':') => {
                                *i += 1;
                                parse(chars, i, out, len, stops, true);
                            }
                            Some('|') => {
                                // Choice: the first option is the default.
                                *i += 1;
                                let mut first = true;
                                while *i < chars.len() && chars[*i] != '|' {
                                    if chars[*i] == ',' {
                                        first = false;
                                    } else if first {
                                        push(out, len, chars[*i]);
                                    }
                                    *i += 1;
                                }
                                *i += 1; // '|'
                            }
                            _ => {}
                        }
                        if *i < chars.len() && chars[*i] == '}' {
                            *i += 1;
                        }
                        stops.push((index, start, *len));
                    } else {
                        // `${VAR:default}`: variables are not known; keep the default.
                        while *i < chars.len() && chars[*i] != ':' && chars[*i] != '}' {
                            *i += 1;
                        }
                        if *i < chars.len() && chars[*i] == ':' {
                            *i += 1;
                            parse(chars, i, out, len, stops, true);
                        }
                        if *i < chars.len() && chars[*i] == '}' {
                            *i += 1;
                        }
                    }
                } else if *i < chars.len() && (chars[*i].is_ascii_alphabetic() || chars[*i] == '_')
                {
                    // `$VAR`: unknown variable, dropped.
                    while *i < chars.len()
                        && (chars[*i].is_ascii_alphanumeric() || chars[*i] == '_')
                    {
                        *i += 1;
                    }
                } else {
                    push(out, len, '$');
                }
            }
            _ => {
                push(out, len, c);
                *i += 1;
            }
        }
    }
}

fn number(chars: &[char], i: &mut usize) -> u32 {
    let mut n = 0u32;
    while *i < chars.len() && chars[*i].is_ascii_digit() {
        n = n * 10 + chars[*i].to_digit(10).unwrap_or(0);
        *i += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_become_stops_in_order() {
        let e = expand("str_replace(${1:\\$search}, ${2:\\$replace}, $3)$0");
        assert_eq!(e.text, "str_replace($search, $replace, )");
        assert_eq!(e.stops, vec![(12, 19), (21, 29), (31, 31), (32, 32)]);
    }

    #[test]
    fn nested_choice_and_variables() {
        let e = expand("${1:outer ${2:inner}} ${3|public,private|} $TM_FILENAME${CLIPBOARD:x}");
        assert_eq!(e.text, "outer inner public x");
        assert_eq!(e.stops[0], (0, 11));
        assert_eq!(e.stops[1], (6, 11));
        assert_eq!(e.stops[2], (12, 18));
    }

    #[test]
    fn plain_text_has_no_stops() {
        let e = expand("count");
        assert_eq!(e.text, "count");
        assert!(e.stops.is_empty());
    }
}
