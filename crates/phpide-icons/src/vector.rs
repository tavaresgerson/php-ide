//! SVG → stroke polylines in the 24×24 icon space.
//!
//! Supports what Tabler outline icons use: `path` (every command, arcs
//! included), `circle`, `ellipse`, `line`, `polyline`, `polygon` and `rect`.
//! Elements with `stroke="none"` (Tabler's bounding box) are skipped.
//! Zero-length subpaths (`M12 8h.01`) become single-point paths, which the
//! painter draws as round dots.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::OnceLock;

use crate::{svg, IconId};

/// One stroked polyline in icon units (0..24).
#[derive(Debug, Clone, PartialEq)]
pub struct IconPath {
    pub points: Vec<[f32; 2]>,
    pub closed: bool,
}

impl IconPath {
    /// A dot: the stroke of a zero-length segment with round caps.
    pub fn is_dot(&self) -> bool {
        self.points.len() == 1
    }
}

static PATHS: OnceLock<HashMap<IconId, Vec<IconPath>>> = OnceLock::new();

/// Parsed polylines of an icon (cached for the process lifetime).
pub fn paths(id: IconId) -> &'static [IconPath] {
    PATHS
        .get_or_init(|| {
            IconId::catalog()
                .iter()
                .chain(std::iter::once(&IconId::Unknown))
                .map(|id| (*id, parse_svg(svg(*id))))
                .collect()
        })
        .get(&id)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn attr(node: &roxmltree::Node, name: &str) -> f32 {
    node.attribute(name)
        .and_then(|value| value.trim_end_matches("px").parse().ok())
        .unwrap_or(0.0)
}

pub(crate) fn parse_svg(source: &str) -> Vec<IconPath> {
    let Ok(document) = roxmltree::Document::parse(source) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in document.descendants().filter(|node| node.is_element()) {
        if node.attribute("stroke") == Some("none") {
            continue;
        }
        match node.tag_name().name() {
            "path" => {
                if let Some(data) = node.attribute("d") {
                    out.extend(parse_path(data));
                }
            }
            "circle" => {
                let r = attr(&node, "r");
                out.push(ellipse(attr(&node, "cx"), attr(&node, "cy"), r, r));
            }
            "ellipse" => out.push(ellipse(
                attr(&node, "cx"),
                attr(&node, "cy"),
                attr(&node, "rx"),
                attr(&node, "ry"),
            )),
            "line" => out.push(IconPath {
                points: vec![
                    [attr(&node, "x1"), attr(&node, "y1")],
                    [attr(&node, "x2"), attr(&node, "y2")],
                ],
                closed: false,
            }),
            "polyline" | "polygon" => {
                let numbers = Numbers::new(node.attribute("points").unwrap_or("")).collect_all();
                let points: Vec<[f32; 2]> = numbers.as_chunks::<2>().0.to_vec();
                if !points.is_empty() {
                    out.push(IconPath {
                        points,
                        closed: node.tag_name().name() == "polygon",
                    });
                }
            }
            "rect" => {
                let (x, y) = (attr(&node, "x"), attr(&node, "y"));
                let (w, h) = (attr(&node, "width"), attr(&node, "height"));
                let r = attr(&node, "rx")
                    .max(attr(&node, "ry"))
                    .min(w / 2.0)
                    .min(h / 2.0);
                out.push(rounded_rect(x, y, w, h, r));
            }
            _ => {}
        }
    }
    out
}

fn segments_for(length: f32) -> usize {
    ((length / 0.75).ceil() as usize).clamp(2, 32)
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> IconPath {
    let n = segments_for(TAU * rx.max(ry)).max(12);
    IconPath {
        points: (0..n)
            .map(|i| {
                let t = TAU * i as f32 / n as f32;
                [cx + rx * t.cos(), cy + ry * t.sin()]
            })
            .collect(),
        closed: true,
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> IconPath {
    if r <= 0.0 {
        return IconPath {
            points: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
            closed: true,
        };
    }
    let mut points = Vec::new();
    let corners = [
        (x + w - r, y + r, -0.25),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, 0.25),
        (x + r, y + r, 0.5),
    ];
    for (cx, cy, start) in corners {
        for step in 0..=4 {
            let t = TAU * (start + step as f32 / 16.0);
            points.push([cx + r * t.cos(), cy + r * t.sin()]);
        }
    }
    IconPath {
        points,
        closed: true,
    }
}

/// Number scanner tolerant of SVG's compact syntax (`1.5.5`, `-1-2`, `1e-3`).
struct Numbers<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Numbers<'a> {
    fn new(source: &'a str) -> Self {
        Numbers {
            bytes: source.as_bytes(),
            pos: 0,
        }
    }

    fn skip_separators(&mut self) {
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_whitespace() || self.bytes[self.pos] == b',')
        {
            self.pos += 1;
        }
    }

    fn peek_command(&mut self) -> Option<u8> {
        self.skip_separators();
        let byte = *self.bytes.get(self.pos)?;
        (byte.is_ascii_alphabetic() && byte != b'e' && byte != b'E').then_some(byte)
    }

    fn has_number(&mut self) -> bool {
        self.skip_separators();
        matches!(self.bytes.get(self.pos), Some(b) if b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.'))
    }

    /// Arc flags may be written without separators (`a1 1 0 01 2 2`).
    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        let byte = *self.bytes.get(self.pos)?;
        self.pos += 1;
        match byte {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_separators();
        let start = self.pos;
        let bytes = self.bytes;
        let mut pos = self.pos;
        if matches!(bytes.get(pos), Some(b'-' | b'+')) {
            pos += 1;
        }
        let mut seen_dot = false;
        let mut seen_digit = false;
        while let Some(&byte) = bytes.get(pos) {
            if byte.is_ascii_digit() {
                seen_digit = true;
            } else if byte == b'.' && !seen_dot {
                seen_dot = true;
            } else {
                break;
            }
            pos += 1;
        }
        if seen_digit && matches!(bytes.get(pos), Some(b'e' | b'E')) {
            let mut exp = pos + 1;
            if matches!(bytes.get(exp), Some(b'-' | b'+')) {
                exp += 1;
            }
            if bytes.get(exp).is_some_and(u8::is_ascii_digit) {
                pos = exp;
                while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
                    pos += 1;
                }
            }
        }
        if !seen_digit {
            return None;
        }
        self.pos = pos;
        std::str::from_utf8(&bytes[start..pos]).ok()?.parse().ok()
    }

    fn collect_all(mut self) -> Vec<f32> {
        let mut out = Vec::new();
        while let Some(value) = self.number() {
            out.push(value);
        }
        out
    }
}

struct Builder {
    out: Vec<IconPath>,
    points: Vec<[f32; 2]>,
    /// Whether the current subpath has a drawing command (a lone `M` draws
    /// nothing, while `M x y h0` draws a dot).
    drawn: bool,
}

impl Builder {
    fn move_to(&mut self, point: [f32; 2]) {
        self.finish(false);
        self.points.push(point);
    }

    fn line_to(&mut self, point: [f32; 2]) {
        if self.points.is_empty() {
            self.points.push(point);
        }
        self.points.push(point);
        self.drawn = true;
    }

    fn finish(&mut self, closed: bool) {
        let points = std::mem::take(&mut self.points);
        let drawn = std::mem::replace(&mut self.drawn, false);
        if points.is_empty() || !drawn {
            return;
        }
        let extent = points.iter().fold(0.0_f32, |acc, p| {
            acc.max((p[0] - points[0][0]).abs())
                .max((p[1] - points[0][1]).abs())
        });
        if extent < 0.1 {
            self.out.push(IconPath {
                points: vec![points[0]],
                closed: false,
            });
        } else if points.len() >= 2 {
            self.out.push(IconPath { points, closed });
        }
    }
}

fn cubic(b: &mut Builder, p0: [f32; 2], c1: [f32; 2], c2: [f32; 2], p3: [f32; 2]) {
    let length = dist(p0, c1) + dist(c1, c2) + dist(c2, p3);
    let n = segments_for(length);
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let point = [
            u * u * u * p0[0]
                + 3.0 * u * u * t * c1[0]
                + 3.0 * u * t * t * c2[0]
                + t * t * t * p3[0],
            u * u * u * p0[1]
                + 3.0 * u * u * t * c1[1]
                + 3.0 * u * t * t * c2[1]
                + t * t * t * p3[1],
        ];
        b.line_to(point);
    }
}

fn quadratic(b: &mut Builder, p0: [f32; 2], c: [f32; 2], p2: [f32; 2]) {
    let n = segments_for(dist(p0, c) + dist(c, p2));
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        b.line_to([
            u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p2[0],
            u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p2[1],
        ]);
    }
}

/// Endpoint-parameterized elliptical arc (SVG 1.1, appendix F.6.5).
#[allow(clippy::too_many_arguments)]
fn arc(
    b: &mut Builder,
    from: [f32; 2],
    mut rx: f32,
    mut ry: f32,
    rotation_deg: f32,
    large: bool,
    sweep: bool,
    to: [f32; 2],
) {
    if dist(from, to) < 1e-6 {
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    if rx < 1e-6 || ry < 1e-6 {
        b.line_to(to);
        return;
    }
    let phi = rotation_deg.to_radians();
    let (sin, cos) = phi.sin_cos();
    let dx2 = (from[0] - to[0]) / 2.0;
    let dy2 = (from[1] - to[1]) / 2.0;
    let x1p = cos * dx2 + sin * dy2;
    let y1p = -sin * dx2 + cos * dy2;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let numerator = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let denominator = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coefficient = (numerator / denominator).max(0.0).sqrt();
    if large == sweep {
        coefficient = -coefficient;
    }
    let cxp = coefficient * rx * y1p / ry;
    let cyp = -coefficient * ry * x1p / rx;
    let cx = cos * cxp - sin * cyp + (from[0] + to[0]) / 2.0;
    let cy = sin * cxp + cos * cyp + (from[1] + to[1]) / 2.0;
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let start = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= TAU;
    } else if sweep && delta < 0.0 {
        delta += TAU;
    }
    let n = segments_for(delta.abs() * rx.max(ry));
    for i in 1..n {
        let t = start + delta * i as f32 / n as f32;
        let (st, ct) = t.sin_cos();
        b.line_to([
            cx + rx * ct * cos - ry * st * sin,
            cy + rx * ct * sin + ry * st * cos,
        ]);
    }
    b.line_to(to);
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

pub(crate) fn parse_path(data: &str) -> Vec<IconPath> {
    let mut numbers = Numbers::new(data);
    let mut b = Builder {
        out: Vec::new(),
        points: Vec::new(),
        drawn: false,
    };
    let mut current = [0.0_f32; 2];
    let mut start = current;
    // Reflection points for S/s and T/t.
    let mut last_cubic: Option<[f32; 2]> = None;
    let mut last_quad: Option<[f32; 2]> = None;
    let mut command = b'M';
    loop {
        if let Some(next) = numbers.peek_command() {
            command = next;
            numbers.pos += 1;
        } else if !numbers.has_number() {
            break;
        } else if command == b'M' {
            command = b'L';
        } else if command == b'm' {
            command = b'l';
        }
        let relative = command.is_ascii_lowercase();
        let base = if relative { current } else { [0.0, 0.0] };
        let point = |numbers: &mut Numbers| -> Option<[f32; 2]> {
            Some([numbers.number()? + base[0], numbers.number()? + base[1]])
        };
        let (mut next_cubic, mut next_quad) = (None, None);
        match command.to_ascii_uppercase() {
            b'M' => {
                let Some(p) = point(&mut numbers) else { break };
                b.move_to(p);
                current = p;
                start = p;
            }
            b'L' => {
                let Some(p) = point(&mut numbers) else { break };
                b.line_to(p);
                current = p;
            }
            b'H' => {
                let Some(x) = numbers.number() else { break };
                current[0] = x + base[0];
                b.line_to(current);
            }
            b'V' => {
                let Some(y) = numbers.number() else { break };
                current[1] = y + base[1];
                b.line_to(current);
            }
            b'C' => {
                let (Some(c1), Some(c2), Some(p)) = (
                    point(&mut numbers),
                    point(&mut numbers),
                    point(&mut numbers),
                ) else {
                    break;
                };
                cubic(&mut b, current, c1, c2, p);
                next_cubic = Some(c2);
                current = p;
            }
            b'S' => {
                let (Some(c2), Some(p)) = (point(&mut numbers), point(&mut numbers)) else {
                    break;
                };
                let c1 = last_cubic
                    .map(|c| [2.0 * current[0] - c[0], 2.0 * current[1] - c[1]])
                    .unwrap_or(current);
                cubic(&mut b, current, c1, c2, p);
                next_cubic = Some(c2);
                current = p;
            }
            b'Q' => {
                let (Some(c), Some(p)) = (point(&mut numbers), point(&mut numbers)) else {
                    break;
                };
                quadratic(&mut b, current, c, p);
                next_quad = Some(c);
                current = p;
            }
            b'T' => {
                let Some(p) = point(&mut numbers) else { break };
                let c = last_quad
                    .map(|c| [2.0 * current[0] - c[0], 2.0 * current[1] - c[1]])
                    .unwrap_or(current);
                quadratic(&mut b, current, c, p);
                next_quad = Some(c);
                current = p;
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(rotation), Some(large), Some(sweep)) = (
                    numbers.number(),
                    numbers.number(),
                    numbers.number(),
                    numbers.flag(),
                    numbers.flag(),
                ) else {
                    break;
                };
                let Some(p) = point(&mut numbers) else { break };
                arc(&mut b, current, rx, ry, rotation, large, sweep, p);
                current = p;
            }
            b'Z' => {
                b.finish(true);
                current = start;
                b.points.push(start);
            }
            _ => break,
        }
        last_cubic = next_cubic;
        last_quad = next_quad;
    }
    b.finish(false);
    b.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(paths: &[IconPath]) -> [f32; 4] {
        paths.iter().flat_map(|p| p.points.iter()).fold(
            [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
            |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            },
        )
    }

    #[test]
    fn every_icon_parses_inside_the_canvas() {
        for id in IconId::catalog() {
            let parsed = paths(*id);
            assert!(!parsed.is_empty(), "{} has no strokes", id.slug());
            let [x0, y0, x1, y1] = bounds(parsed);
            assert!(
                x0 >= -0.5 && y0 >= -0.5 && x1 <= 24.5 && y1 <= 24.5,
                "{} leaves the 24×24 canvas: {:?}",
                id.slug(),
                (x0, y0, x1, y1)
            );
            assert!(
                x1 - x0 > 4.0 || y1 - y0 > 4.0,
                "{} is degenerate",
                id.slug()
            );
        }
        assert!(!paths(IconId::Unknown).is_empty());
    }

    #[test]
    fn bounding_box_path_is_skipped_and_dots_are_kept() {
        let info = paths(IconId::Info);
        assert!(info.iter().any(IconPath::is_dot), "{info:?}");
        assert!(
            bounds(info)[0] > 0.5,
            "Tabler's 0..24 frame must be ignored"
        );
    }

    #[test]
    fn arcs_flatten_to_the_right_circle() {
        // Tabler's circle idiom: two half arcs.
        let parsed = parse_path("M10 12a2 2 0 1 0 4 0a2 2 0 0 0 -4 0");
        let points: Vec<_> = parsed.iter().flat_map(|p| p.points.clone()).collect();
        assert!(points.len() > 8);
        for p in points {
            let r = ((p[0] - 12.0).powi(2) + (p[1] - 12.0).powi(2)).sqrt();
            assert!((r - 2.0).abs() < 0.01, "{p:?} r={r}");
        }
    }

    #[test]
    fn compact_numbers_and_implicit_commands() {
        let parsed = parse_path("M1-2.5.5 0l1 1 2 2h.01z");
        assert_eq!(parsed[0].points[0], [1.0, -2.5]);
        assert_eq!(parsed[0].points[1], [0.5, 0.0]);
        assert_eq!(parsed[0].points[2], [1.5, 1.0]);
        assert_eq!(parsed[0].points[3], [3.5, 3.0]);
        assert!(parsed[0].closed);
    }
}
