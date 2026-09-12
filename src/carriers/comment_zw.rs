//! Zero-width character carrier inside comments (document order).

use anyhow::{Result, bail};

use super::Carrier;
use crate::bits::{BitReader, BitWriter};
use crate::carriers::CarrierKind;
use crate::lang::{LangProfile, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZwAlphabet {
    /// U+200B, U+200C, U+200D, U+2060 — 2 bits each
    Zw4,
    /// U+FE00..FE0F — 4 bits each
    Vs16,
}

impl ZwAlphabet {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "zw4" => Ok(Self::Zw4),
            "vs16" => Ok(Self::Vs16),
            other => bail!("unknown zw alphabet: {other}"),
        }
    }

    pub fn bits_per_symbol(self) -> usize {
        match self {
            Self::Zw4 => 2,
            Self::Vs16 => 4,
        }
    }

    pub fn symbols(self) -> &'static [char] {
        match self {
            Self::Zw4 => &['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}'],
            Self::Vs16 => &[
                '\u{FE00}', '\u{FE01}', '\u{FE02}', '\u{FE03}', '\u{FE04}', '\u{FE05}', '\u{FE06}',
                '\u{FE07}', '\u{FE08}', '\u{FE09}', '\u{FE0A}', '\u{FE0B}', '\u{FE0C}', '\u{FE0D}',
                '\u{FE0E}', '\u{FE0F}',
            ],
        }
    }

    pub fn encode_symbol(self, value: u8) -> char {
        let syms = self.symbols();
        syms[(value as usize) % syms.len()]
    }

    pub fn decode_symbol(self, c: char) -> Option<u8> {
        self.symbols().iter().position(|&s| s == c).map(|i| i as u8)
    }

    pub fn strip_char(c: char) -> bool {
        matches!(
            c,
            '\u{200B}'
                | '\u{200C}'
                | '\u{200D}'
                | '\u{2060}'
                | '\u{FE00}'..='\u{FE0F}'
        )
    }
}

pub struct CommentZwCarrier {
    pub alphabet: ZwAlphabet,
    pub max_per_comment: usize,
    pub disabled: bool,
}

impl Carrier for CommentZwCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::CommentZw
    }

    fn capacity(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> usize {
        if self.disabled {
            return 0;
        }
        let bps = self.alphabet.bits_per_symbol();
        let mut slots = 0usize;
        for sp in spans.iter().filter(|s| s.is_comment()) {
            if sp.text(src).len() >= 2 {
                slots += self.max_per_comment;
            }
        }
        slots * bps
    }

    fn encode(
        &self,
        src: &str,
        spans: &[Span],
        _profile: &LangProfile,
        bits: &[u8],
        bit_len: usize,
    ) -> Result<String> {
        if self.disabled {
            bail!("comment-zw disabled under --ascii-only");
        }

        let mut reader = BitReader::new(bits);
        let mut bits_written = 0usize;
        let mut out = String::with_capacity(src.len() + bit_len * 3);
        let mut last = 0usize;
        let bps = self.alphabet.bits_per_symbol();

        for sp in spans.iter().filter(|s| s.is_comment()) {
            out.push_str(&src[last..sp.start]);
            let comment = &src[sp.start..sp.end];
            let stripped: String = comment
                .chars()
                .filter(|c| !ZwAlphabet::strip_char(*c))
                .collect();

            let symbols_here = if bits_written >= bit_len {
                0
            } else {
                let remaining_syms = (bit_len - bits_written).div_ceil(bps);
                remaining_syms.min(self.max_per_comment)
            };

            let mut zw = String::new();
            for _ in 0..symbols_here {
                zw.push(self.take_symbol(&mut reader, &mut bits_written, bit_len, bps));
            }

            out.push_str(&inject_zw(&stripped, &zw));
            last = sp.end;
        }
        out.push_str(&src[last..]);

        if bits_written < bit_len {
            bail!(
                "comment-zw: only wrote {bits_written}/{bit_len} bits (need more comments or raise --zw-per-comment)"
            );
        }
        Ok(out)
    }

    fn decode(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> Result<Vec<u8>> {
        if self.disabled {
            bail!("comment-zw disabled");
        }
        let mut w = BitWriter::new();
        let bps = self.alphabet.bits_per_symbol();
        for sp in spans.iter().filter(|s| s.is_comment()) {
            for c in sp.text(src).chars() {
                if let Some(val) = self.alphabet.decode_symbol(c) {
                    w.write_bits(val as u64, bps);
                }
            }
        }
        Ok(w.finish())
    }

    fn strip(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> Result<String> {
        let mut out = String::with_capacity(src.len());
        let mut last = 0usize;
        for sp in spans.iter().filter(|s| s.is_comment()) {
            out.push_str(&src[last..sp.start]);
            for c in src[sp.start..sp.end].chars() {
                if !ZwAlphabet::strip_char(c) {
                    out.push(c);
                }
            }
            last = sp.end;
        }
        out.push_str(&src[last..]);
        Ok(out)
    }
}

impl CommentZwCarrier {
    fn take_symbol(
        &self,
        reader: &mut BitReader,
        bits_written: &mut usize,
        bit_len: usize,
        bps: usize,
    ) -> char {
        let take = bps.min(bit_len - *bits_written);
        let mut val = 0u8;
        for _ in 0..take {
            val <<= 1;
            if reader.read_bit().unwrap_or(false) {
                val |= 1;
            }
            *bits_written += 1;
        }
        if take < bps {
            val <<= (bps - take) as u8;
        }
        self.alphabet.encode_symbol(val)
    }
}

/// Insert zero-width payload before block closer or at end of line comment.
fn inject_zw(comment: &str, zw: &str) -> String {
    if zw.is_empty() {
        return comment.to_string();
    }
    if let Some(stripped) = comment.strip_suffix("*/") {
        format!("{stripped}{zw}*/")
    } else if let Some(stripped) = comment.strip_suffix("-->") {
        format!("{stripped}{zw}-->")
    } else if let Some(stripped) = comment.strip_suffix("--!>") {
        format!("{stripped}{zw}--!>")
    } else {
        format!("{comment}{zw}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_before_c_and_markup_closers() {
        assert_eq!(inject_zw("/* foo */", "Z"), "/* foo Z*/");
        assert_eq!(inject_zw("<!-- foo -->", "Z"), "<!-- foo Z-->");
        assert_eq!(inject_zw("<!-- foo --!>", "Z"), "<!-- foo Z--!>");
        assert_eq!(inject_zw("// foo", "Z"), "// fooZ");
    }
}
