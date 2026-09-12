//! End-of-line trailing whitespace carrier.

use anyhow::{Result, bail};

use super::Carrier;
use crate::bits::{BitReader, BitWriter};
use crate::carriers::CarrierKind;
use crate::lang::LangProfile;
use crate::lang::{Span, SpanKind};

pub struct EolCarrier {
    pub bits_per_line: u8,
}

impl Carrier for EolCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::Eol
    }

    fn capacity(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> usize {
        eligible_line_ends(src, spans).len() * self.bits_per_line as usize
    }

    fn encode(
        &self,
        src: &str,
        spans: &[Span],
        _profile: &LangProfile,
        bits: &[u8],
        bit_len: usize,
    ) -> Result<String> {
        let ends = eligible_line_ends(src, spans);
        let needed_lines = (bit_len + self.bits_per_line as usize - 1) / self.bits_per_line as usize;
        if ends.len() < needed_lines {
            bail!(
                "eol: need {needed_lines} eligible lines, have {}",
                ends.len()
            );
        }

        // Build set of line-end byte positions we will rewrite
        let mut reader = BitReader::new(bits);
        let mut written = 0usize;

        // Map: position of EOL span start → trailing whitespace to insert before it
        let mut trail_at: Vec<(usize, String)> = Vec::new();
        for end_pos in ends {
            if written >= bit_len {
                // normalize: no trailing ws
                trail_at.push((end_pos, String::new()));
                continue;
            }
            let bps = self.bits_per_line as usize;
            let take = bps.min(bit_len - written);
            let mut val = 0u8;
            for _ in 0..take {
                val <<= 1;
                if reader.read_bit().unwrap_or(false) {
                    val |= 1;
                }
                written += 1;
            }
            let trail = match self.bits_per_line {
                1 => {
                    if val & 1 == 1 {
                        " ".to_string()
                    } else {
                        String::new()
                    }
                }
                2 => match val & 0b11 {
                    0b00 => String::new(),
                    0b01 => " ".to_string(),
                    0b10 => "\t".to_string(),
                    _ => "  ".to_string(),
                },
                _ => {
                    if val != 0 {
                        " ".to_string()
                    } else {
                        String::new()
                    }
                }
            };
            trail_at.push((end_pos, trail));
        }

        // Reconstruct: for each line, strip existing trailing ws before EOL, insert new
        let mut out = String::with_capacity(src.len() + bit_len);
        let mut last = 0usize;
        let mut trail_idx = 0usize;

        for sp in spans {
            if sp.kind == SpanKind::Eol {
                // content from last to eol — trim trailing spaces/tabs on this line
                let line_chunk = &src[last..sp.start];
                let trimmed = line_chunk.trim_end_matches([' ', '\t']);
                out.push_str(trimmed);
                if trail_idx < trail_at.len() && trail_at[trail_idx].0 == sp.start {
                    out.push_str(&trail_at[trail_idx].1);
                    trail_idx += 1;
                }
                out.push_str(&src[sp.start..sp.end]);
                last = sp.end;
            }
        }
        // remainder (file may lack final newline)
        if last < src.len() {
            let rest = &src[last..];
            // If there's a final line without EOL that we marked — handle via ends that point past last eol
            let trimmed = rest.trim_end_matches([' ', '\t']);
            out.push_str(trimmed);
            // check if any trail targets the end of file as a virtual line end
            while trail_idx < trail_at.len() {
                if trail_at[trail_idx].0 >= last {
                    out.push_str(&trail_at[trail_idx].1);
                }
                trail_idx += 1;
            }
            // if original had content after trim that wasn't only ws, keep non-ws — already trimmed
        }

        if written < bit_len {
            bail!("eol: only wrote {written}/{bit_len} bits");
        }
        Ok(out)
    }

    fn decode(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> Result<Vec<u8>> {
        let mut w = BitWriter::new();
        for end_pos in eligible_line_ends(src, spans) {
            // trailing ws immediately before end_pos
            let mut i = end_pos;
            while i > 0 && (src.as_bytes()[i - 1] == b' ' || src.as_bytes()[i - 1] == b'\t') {
                i -= 1;
            }
            let trail = &src[i..end_pos];
            match self.bits_per_line {
                1 => {
                    w.write_bit(!trail.is_empty());
                }
                2 => {
                    let val: u8 = if trail.is_empty() {
                        0b00
                    } else if trail == " " {
                        0b01
                    } else if trail == "\t" {
                        0b10
                    } else {
                        0b11
                    };
                    w.write_bits(val as u64, 2);
                }
                n => {
                    w.write_bits((!trail.is_empty()) as u64, n as usize);
                }
            }
        }
        Ok(w.finish())
    }

    fn strip(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> Result<String> {
        // Remove all trailing spaces/tabs before EOLs
        let mut out = String::with_capacity(src.len());
        let mut last = 0usize;
        for sp in spans {
            if sp.kind == SpanKind::Eol {
                let chunk = &src[last..sp.start];
                out.push_str(chunk.trim_end_matches([' ', '\t']));
                out.push_str(&src[sp.start..sp.end]);
                last = sp.end;
            }
        }
        if last < src.len() {
            out.push_str(src[last..].trim_end_matches([' ', '\t']));
        }
        Ok(out)
    }
}

/// Byte positions of line endings that are safe to mark (not inside strings).
fn eligible_line_ends(src: &str, spans: &[Span]) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut in_string = false;
    for sp in spans {
        match sp.kind {
            SpanKind::String => in_string = true,
            SpanKind::Eol => {
                if !in_string {
                    ends.push(sp.start);
                }
                // strings don't span EOLs in our lexer for normal quotes;
                // raw/template may contain EOLs as part of String span, so
                // those EOLs aren't separate Eol spans.
                in_string = false;
            }
            SpanKind::Code | SpanKind::Whitespace | SpanKind::LineComment | SpanKind::BlockComment
            | SpanKind::Skipped => {
                in_string = false;
            }
        }
    }
    // Final line without newline
    if !src.is_empty() && !src.ends_with('\n') && !src.ends_with('\r') {
        // ensure we're not inside a string at EOF
        let last_string = spans
            .iter()
            .rev()
            .find(|s| matches!(s.kind, SpanKind::String | SpanKind::Code | SpanKind::LineComment | SpanKind::BlockComment | SpanKind::Skipped | SpanKind::Whitespace));
        if let Some(sp) = last_string {
            if sp.kind != SpanKind::String && sp.end == src.len() {
                ends.push(src.len());
            } else if sp.kind != SpanKind::String {
                ends.push(src.len());
            }
        } else {
            ends.push(src.len());
        }
    }
    ends
}
