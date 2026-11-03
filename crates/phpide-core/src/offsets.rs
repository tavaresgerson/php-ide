//! Conversões centralizadas de posição — `T-NFR-002-01` (NFR-002).
//!
//! A rope é a fonte de verdade do texto; todo o resto (byte offsets do
//! tree-sitter, índices de caractere do egui, unidades UTF-16 do LSP,
//! linha:coluna da status bar) é uma *visão* sobre ela. NFR-002 exige que
//! essas conversões vivam num único lugar, testado com Unicode e CRLF.
//!
//! Convenções:
//! - índice de caractere = deslocamento Unicode escalar (0-based), o que o
//!   egui chama de `CCursor.index` e a rope de `char`;
//! - byte = deslocamento UTF-8 (0-based), usado pelo tree-sitter;
//! - linha = 0-based internamente; 1-based só na apresentação;
//! - coluna em unidades do encoding negociado; `\r\n` nunca conta como
//!   duas colunas (a posição entre `\r` e `\n` colapsa para a do `\n`).

use crate::Document;

/// Encoding de posição negociado com um servidor LSP (NFR-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositionEncoding {
    /// Caracteres Unicode escalares (utf-8 no LSP 3.17+).
    Utf8,
    /// Unidades de código UTF-16 (default do LSP).
    Utf16,
    /// Unidades de código UTF-32.
    Utf32,
}

impl PositionEncoding {
    pub const fn label(self) -> &'static str {
        match self {
            PositionEncoding::Utf8 => "utf-8",
            PositionEncoding::Utf16 => "utf-16",
            PositionEncoding::Utf32 => "utf-32",
        }
    }

    /// Número de unidades deste encoding ocupado por `c` (≥ 1).
    fn units(self, c: char) -> usize {
        match self {
            PositionEncoding::Utf8 => c.len_utf8(),
            PositionEncoding::Utf16 => c.len_utf16(),
            PositionEncoding::Utf32 => 1,
        }
    }
}

/// Negociação de encoding client-first: a primeira capacidade do cliente
/// também suportada pelo servidor vence (fallback clássico do LSP 3.17).
/// `None` significa "sem sobreposição" — o chamador deve seguir o default
/// UTF-16 do protocolo.
pub fn negotiate_encoding(
    client_supported: &[PositionEncoding],
    server_supported: &[PositionEncoding],
) -> Option<PositionEncoding> {
    client_supported
        .iter()
        .copied()
        .find(|e| server_supported.contains(e))
}

impl Document {
    /// Deslocamento em bytes UTF-8 do caractere `char_offset`.
    pub fn char_to_byte(&self, char_offset: usize) -> usize {
        self.rope()
            .len_bytes()
            .min(self.rope().char_to_byte(char_offset))
    }

    /// Deslocamento em caracteres do byte UTF-8 `byte_offset` (clamped).
    pub fn byte_to_char(&self, byte_offset: usize) -> usize {
        let rope = self.rope();
        let byte = rope.len_bytes().min(byte_offset);
        rope.byte_to_char(byte)
    }

    /// Unidades do encoding no deslocamento em caracteres `char_offset`
    /// a partir do início do documento.
    pub fn char_to_encoded_units(&self, char_offset: usize, enc: PositionEncoding) -> usize {
        let rope = self.rope();
        let upto = rope.len_chars().min(char_offset);
        rope.slice(0..upto).chars().map(|c| enc.units(c)).sum()
    }

    /// Deslocamento em caracteres correspondente a `units` unidades do
    /// encoding desde o início do documento. Retorna o primeiro caractere
    /// cuja faixa de unidades contém `units` — uma posição dentro de um
    /// par surrogate UTF-16 colapsa para o início dele (nunca quebra o
    /// par); `units` igual ao acumulado exato cai no caractere seguinte.
    pub fn encoded_units_to_char(&self, units: usize, enc: PositionEncoding) -> usize {
        let rope = self.rope();
        let mut acc = 0usize;
        for (i, c) in rope.chars().enumerate() {
            let next = acc + enc.units(c);
            if next > units {
                return i;
            }
            acc = next;
        }
        rope.len_chars()
    }

    /// (linha 0-based, coluna em unidades do encoding) de `char_offset`.
    /// A coluna mede unidades dentro da linha. O `\r` de um terminador
    /// CRLF tem largura zero: a posição entre `\r` e `\n` colapsa com a
    /// do próprio `\n` (mesmo ponto de inserção), como no VS Code.
    pub fn line_col_encoded(&self, char_offset: usize, enc: PositionEncoding) -> (usize, usize) {
        let rope = self.rope();
        let upto = rope.len_chars().min(char_offset);
        let line0 = rope.char_to_line(upto);
        let line_start = rope.line_to_char(line0);
        let mut col = 0usize;
        for c in rope.slice(line_start..upto).chars() {
            if c == '\r' {
                continue;
            }
            col += enc.units(c);
        }
        (line0, col)
    }

    /// Caractere que contém a posição LSP `(line0, character)` no encoding
    /// negociado. `character` é 0-based em unidades do encoding; o `\r` de
    /// CRLF tem largura zero (ver [`Document::line_col_encoded`]).
    pub fn lsp_position_to_char(
        &self,
        line0: usize,
        character: usize,
        enc: PositionEncoding,
    ) -> usize {
        let rope = self.rope();
        if line0 >= rope.len_lines() {
            return rope.len_chars();
        }
        let line_start = rope.line_to_char(line0);
        let mut acc = 0usize;
        for (i, c) in rope.line(line0).chars().enumerate() {
            let width = if c == '\r' { 0 } else { enc.units(c) };
            let next = acc + width;
            if next > character {
                return line_start + i;
            }
            acc = next;
        }
        line_start + rope.line(line0).len_chars()
    }

    /// Posição LSP `(line0, character)` 0-based de `char_offset`.
    pub fn char_to_lsp_position(
        &self,
        char_offset: usize,
        enc: PositionEncoding,
    ) -> (usize, usize) {
        self.line_col_encoded(char_offset, enc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_counts_astral_chars_as_two_units() {
        // 😀 é U+1F600: 1 char, 4 bytes UTF-8, 2 unidades UTF-16.
        let doc = Document::new("a😀b");
        assert_eq!(doc.char_to_byte(1), 1);
        assert_eq!(doc.char_to_byte(2), 5);
        assert_eq!(doc.byte_to_char(5), 2);
        assert_eq!(doc.char_to_encoded_units(3, PositionEncoding::Utf16), 4);
        assert_eq!(doc.encoded_units_to_char(4, PositionEncoding::Utf16), 3);
        // Não quebrar no meio do pair: unidades 1..=3 tocam o 😀.
        assert_eq!(doc.encoded_units_to_char(1, PositionEncoding::Utf16), 1);
        assert_eq!(doc.encoded_units_to_char(2, PositionEncoding::Utf16), 1);
        assert_eq!(doc.encoded_units_to_char(3, PositionEncoding::Utf16), 2);
        // UTF-8 conta bytes; UTF-32 conta chars.
        assert_eq!(doc.char_to_encoded_units(3, PositionEncoding::Utf8), 6);
        assert_eq!(doc.char_to_encoded_units(3, PositionEncoding::Utf32), 3);
    }

    #[test]
    fn crlf_columns_collapse_between_cr_and_lf() {
        let doc = Document::new("ab\r\ncd\r\n");
        // offset 2 = antes do \r: linha 0, col 2.
        assert_eq!(doc.line_col_encoded(2, PositionEncoding::Utf16), (0, 2));
        // offset 3 = o próprio '\n': colapsa para col 2 (mesmo ponto de inserção).
        assert_eq!(doc.line_col_encoded(3, PositionEncoding::Utf16), (0, 2));
        // offset 4 = início da linha 1.
        assert_eq!(doc.line_col_encoded(4, PositionEncoding::Utf16), (1, 0));
        assert_eq!(doc.line_col_encoded(6, PositionEncoding::Utf16), (1, 2));
        assert_eq!(doc.line_col_encoded(7, PositionEncoding::Utf16), (1, 2));
        // Caminho de volta: a posição (0, 2) representa o '\n' (representante
        // do par CRLF colapsado); (1, 2) representa o '\n' da linha 1.
        assert_eq!(doc.lsp_position_to_char(0, 2, PositionEncoding::Utf16), 3);
        assert_eq!(doc.lsp_position_to_char(1, 0, PositionEncoding::Utf16), 4);
        assert_eq!(doc.lsp_position_to_char(1, 2, PositionEncoding::Utf16), 7);
        assert_eq!(doc.lsp_position_to_char(0, 0, PositionEncoding::Utf16), 0);
    }

    #[test]
    fn mixed_eol_and_unicode_roundtrip() {
        let text = "héllo\nwörld 😀\r\nplain";
        let doc = Document::new(text);
        for enc in [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ] {
            for char_at in 0..=doc.len_chars() {
                let (line, col) = doc.char_to_lsp_position(char_at, enc);
                let back = doc.lsp_position_to_char(line, col, enc);
                // O '\r' de um CRLF colapsa com o '\n' seguinte: o
                // representante da posição é o próprio '\n'.
                let representative = if char_at < doc.len_chars()
                    && doc.char(char_at) == '\r'
                    && char_at + 1 < doc.len_chars()
                    && doc.char(char_at + 1) == '\n'
                {
                    char_at + 1
                } else {
                    char_at
                };
                assert_eq!(
                    back, representative,
                    "roundtrip falhou em {char_at} ({enc:?})"
                );
            }
        }
    }

    #[test]
    fn negotiate_picks_first_common_client_preference() {
        use PositionEncoding::*;
        assert_eq!(
            negotiate_encoding(&[Utf8, Utf16], &[Utf16, Utf8]),
            Some(Utf8)
        );
        assert_eq!(negotiate_encoding(&[Utf8], &[Utf16]), None);
        assert_eq!(negotiate_encoding(&[Utf16], &[Utf16]), Some(Utf16));
        assert_eq!(
            negotiate_encoding(&[Utf8, Utf16], &[Utf32, Utf16]),
            Some(Utf16)
        );
    }

    #[test]
    fn line_col_matches_existing_display_semantics() {
        let doc = Document::new("abc\ndef\n");
        // Compatível com Document::line_col (1-based) para texto LF simples.
        let (l1, c1) = doc.line_col(5);
        let (l0, c0) = doc.line_col_encoded(5, PositionEncoding::Utf32);
        assert_eq!((l0 + 1, c0 + 1), (l1, c1));
    }
}
