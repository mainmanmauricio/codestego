//! End-of-file carrier: whitespace lines or a trailing block comment.

use anyhow::{Result, bail};

use super::Carrier;
use crate::bits::{BitReader, BitWriter};
use crate::carriers::comment_zw::ZwAlphabet;
use crate::carriers::CarrierKind;
use crate::lang::LangProfile;
use crate::lang::Span;

pub struct EofCarrier {
    pub lines: usize,
    pub as_comment: bool,
    pub alphabet: ZwAlphabet,
    pub ascii_only: bool,
}

impl Carrier for EofCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::Eof
    }

    fn capacity(&self, _src: &str, _spans: &[Span], _profile: &LangProfile) -> usize {
        if self.as_comment {
            if self.ascii_only {
                // ASCII comment with space encoding: we can make it as long as needed
                // Report a large capacity; actual encode will size it.
                return 8192;
            }
            // ZW in one comment: unbounded in principle; report generous capacity
            return 8192;
        }
        // Each eof line encodes 8 bits
        let lines = if self.lines == 0 { 256 } else { self.lines };
        lines * 8
    }

    fn encode(
        &self,
        src: &str,
        _spans: &[Span],
        profile: &LangProfile,
        bits: &[u8],
        bit_len: usize,
    ) -> Result<String> {
        // Strip existing eof payload first
        let base = strip_eof_payload(src, profile);

        if self.as_comment {
            return self.encode_comment(&base, bits, bit_len, profile);
        }

        let lines_needed = (bit_len + 7) / 8;
        let lines = if self.lines == 0 {
            lines_needed
        } else {
            self.lines
        };
        if lines < lines_needed {
            bail!("eof: need {lines_needed} lines, --eof-lines is {lines}");
        }

        let mut reader = BitReader::new(bits);
        let mut out = base;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        for i in 0..lines {
            let mut byte = 0u8;
            if i * 8 < bit_len {
                let take = 8.min(bit_len - i * 8);
                for _ in 0..take {
                    byte <<= 1;
                    if reader.read_bit().unwrap_or(false) {
                        byte |= 1;
                    }
                }
                if take < 8 {
                    byte <<= (8 - take) as u8;
                }
            }
            // Encode byte as that many trailing spaces on an otherwise empty line
            // Use a mix: line is "\u{200B}".repeat isn't ascii — use spaces only
            for _ in 0..byte {
                out.push(' ');
            }
            out.push('\n');
        }
        Ok(out)
    }

    fn decode(&self, src: &str, _spans: &[Span], profile: &LangProfile) -> Result<Vec<u8>> {
        if self.as_comment {
            return self.decode_comment(src, profile);
        }
        // Read trailing empty lines with only spaces
        let mut w = BitWriter::new();
        let lines: Vec<&str> = src.lines().collect();
        // Find run of trailing space-only lines
        let mut trailing = Vec::new();
        for line in lines.iter().rev() {
            if line.chars().all(|c| c == ' ') {
                trailing.push(*line);
            } else {
                break;
            }
        }
        trailing.reverse();
        for line in trailing {
            let byte = line.len().min(255) as u8;
            w.write_bits(byte as u64, 8);
        }
        Ok(w.finish())
    }

    fn strip(&self, src: &str, _spans: &[Span], profile: &LangProfile) -> Result<String> {
        Ok(strip_eof_payload(src, profile))
    }
}

impl EofCarrier {
    fn encode_comment(
        &self,
        base: &str,
        bits: &[u8],
        bit_len: usize,
        profile: &LangProfile,
    ) -> Result<String> {
        let (opener, closer) = profile.eof_comment_delimiters();
        let mut reader = BitReader::new(bits);
        let mut body = String::from(" codestego ");
        if self.ascii_only {
            // encode as space runs in the comment body
            for i in 0..bit_len {
                let bit = reader.read_bit().unwrap_or(false);
                body.push(if bit { 'x' } else { 'o' });
                // invisible-ish: use space padding between markers
                let _ = i;
                body.push(if bit { ' ' } else { ' ' });
                body.push(' ');
                if bit {
                    body.push(' ');
                }
            }
            // Actually use a clearer ASCII scheme: '0'/'1' is too visible.
            // Use alternating single/double spaces between dots.
            body = String::from(" .");
            let mut reader = BitReader::new(bits);
            for _ in 0..bit_len {
                let bit = reader.read_bit().unwrap_or(false);
                if bit {
                    body.push_str("  ");
                } else {
                    body.push(' ');
                }
                body.push('.');
            }
            body.push(' ');
        } else {
            let bps = self.alphabet.bits_per_symbol();
            let mut written = 0usize;
            while written < bit_len {
                let take = bps.min(bit_len - written);
                let mut val = 0u8;
                for _ in 0..take {
                    val <<= 1;
                    if reader.read_bit().unwrap_or(false) {
                        val |= 1;
                    }
                    written += 1;
                }
                if take < bps {
                    val <<= (bps - take) as u8;
                }
                body.push(self.alphabet.encode_symbol(val));
            }
        }
        let mut out = base.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(opener);
        out.push_str(&body);
        out.push_str(closer);
        out.push('\n');
        Ok(out)
    }

    fn decode_comment(&self, src: &str, profile: &LangProfile) -> Result<Vec<u8>> {
        let (opener, closer) = profile.eof_comment_delimiters();
        let Some(start) = src.rfind(opener) else {
            bail!("eof comment not found");
        };
        let after = start + opener.len();
        let Some(rel_end) = src[after..].find(closer) else {
            bail!("eof comment unclosed");
        };
        let body = &src[after..after + rel_end];
        let mut w = BitWriter::new();
        if self.ascii_only {
            // dots separated by 1 or 2 spaces
            let mut chars = body.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '.' {
                    let mut spaces = 0;
                    while chars.peek() == Some(&' ') {
                        spaces += 1;
                        chars.next();
                    }
                    if chars.peek() == Some(&'.') || spaces > 0 {
                        // bit is whether we had 2 spaces before this next marker
                        // Actually we wrote: '.' + spaces + '.' so after seeing '.', count spaces until next '.'
                        if spaces >= 1 {
                            w.write_bit(spaces >= 2);
                        }
                    }
                }
            }
        } else {
            let bps = self.alphabet.bits_per_symbol();
            for c in body.chars() {
                if let Some(val) = self.alphabet.decode_symbol(c) {
                    w.write_bits(val as u64, bps);
                }
            }
        }
        Ok(w.finish())
    }
}

fn strip_eof_payload(src: &str, profile: &LangProfile) -> String {
    let (opener, closer) = profile.eof_comment_delimiters();
    let mut s = src.to_string();
    // Remove trailing space-only lines
    loop {
        let trimmed = s.trim_end_matches('\n');
        let last_line_start = trimmed.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let last_line = &trimmed[last_line_start..];
        if !last_line.is_empty() && last_line.chars().all(|c| c == ' ') {
            s = trimmed[..last_line_start].to_string();
            continue;
        }
        break;
    }
    if let Some(start) = s.rfind(opener) {
        if let Some(end) = s[start + opener.len()..].find(closer) {
            let body_start = start + opener.len();
            let block_end = body_start + end + closer.len();
            if block_end == s.trim_end().len() || s[block_end..].trim().is_empty() {
                let body = &s[body_start..body_start + end];
                let looks_ours = body.contains("codestego")
                    || body.chars().all(|c| {
                        c.is_whitespace()
                            || matches!(
                                c,
                                '.' | '\u{200B}'
                                    | '\u{200C}'
                                    | '\u{200D}'
                                    | '\u{2060}'
                                    | '\u{FE00}'..='\u{FE0F}'
                            )
                    });
                if looks_ours {
                    s = s[..start].trim_end().to_string();
                    if !s.is_empty() {
                        s.push('\n');
                    }
                }
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carriers::Carrier;
    use crate::lang::Lang;

    #[test]
    fn markup_eof_comment_uses_html_delimiters() {
        let profile = Lang::Html.profile();
        let carrier = EofCarrier {
            lines: 0,
            as_comment: true,
            alphabet: ZwAlphabet::Zw4,
            ascii_only: false,
        };
        let src = "<p>hi</p>\n";
        let out = carrier.encode(src, &[], &profile, &[0b1010_0000], 4).unwrap();
        assert!(out.contains("<!--"), "{out}");
        assert!(out.contains("-->"), "{out}");
        assert!(!out.contains("/*"), "{out}");
        let bits = carrier.decode(&out, &[], &profile).unwrap();
        assert!(!bits.is_empty());
        let stripped = carrier.strip(&out, &[], &profile).unwrap();
        assert!(!stripped.contains("codestego"));
        assert!(stripped.contains("<p>hi</p>"));
    }

    #[test]
    fn c_family_eof_comment_still_block() {
        let profile = Lang::Rust.profile();
        let carrier = EofCarrier {
            lines: 0,
            as_comment: true,
            alphabet: ZwAlphabet::Zw4,
            ascii_only: false,
        };
        let src = "fn main() {}\n";
        let out = carrier.encode(src, &[], &profile, &[0b1010_0000], 4).unwrap();
        assert!(out.contains("/*"), "{out}");
        assert!(out.contains("*/"), "{out}");
        assert!(!out.contains("<!--"), "{out}");
    }
}
