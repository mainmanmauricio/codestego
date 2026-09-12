//! Inter-word space run carrier inside comments (1 space = 0, 2 spaces = 1).

use anyhow::{Result, bail};

use super::Carrier;
use crate::bits::{BitReader, BitWriter};
use crate::carriers::CarrierKind;
use crate::lang::LangProfile;
use crate::lang::Span;

pub struct CommentSpaceCarrier;

impl Carrier for CommentSpaceCarrier {
    fn kind(&self) -> CarrierKind {
        CarrierKind::CommentSpace
    }

    fn capacity(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> usize {
        let mut n = 0usize;
        for sp in spans.iter().filter(|s| s.is_comment()) {
            n += space_runs(sp.text(src)).len();
        }
        n
    }

    fn encode(
        &self,
        src: &str,
        spans: &[Span],
        _profile: &LangProfile,
        bits: &[u8],
        bit_len: usize,
    ) -> Result<String> {
        let mut reader = BitReader::new(bits);
        let mut written = 0usize;
        let mut out = String::with_capacity(src.len() + bit_len);
        let mut last = 0usize;

        for sp in spans.iter().filter(|s| s.is_comment()) {
            out.push_str(&src[last..sp.start]);
            let comment = sp.text(src);
            let rewritten = rewrite_space_runs(comment, &mut reader, &mut written, bit_len);
            out.push_str(&rewritten);
            last = sp.end;
        }
        out.push_str(&src[last..]);

        if written < bit_len {
            bail!("comment-space: only wrote {written}/{bit_len} bits");
        }
        Ok(out)
    }

    fn decode(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> Result<Vec<u8>> {
        let mut w = BitWriter::new();
        for sp in spans.iter().filter(|s| s.is_comment()) {
            for run in space_runs(sp.text(src)) {
                // 1 space => 0, 2+ spaces => 1 (normalize to bit)
                w.write_bit(run.len >= 2);
            }
        }
        Ok(w.finish())
    }

    fn strip(&self, src: &str, spans: &[Span], _profile: &LangProfile) -> Result<String> {
        // Normalize all inter-word runs to single space
        let mut out = String::with_capacity(src.len());
        let mut last = 0usize;
        for sp in spans.iter().filter(|s| s.is_comment()) {
            out.push_str(&src[last..sp.start]);
            out.push_str(&normalize_spaces(sp.text(src)));
            last = sp.end;
        }
        out.push_str(&src[last..]);
        Ok(out)
    }
}

#[derive(Debug)]
struct SpaceRun {
    start: usize,
    len: usize,
}

fn space_runs(comment: &str) -> Vec<SpaceRun> {
    let bytes = comment.as_bytes();
    let (opener, skip_star) = if bytes.starts_with(b"<!--") {
        (4, false)
    } else if bytes.starts_with(b"//") || bytes.starts_with(b"/*") {
        (2, bytes.starts_with(b"/*"))
    } else {
        (0, false)
    };
    let mut runs = Vec::new();
    let mut i = opener;
    // skip leading spaces after opener (not inter-word)
    while i < bytes.len()
        && (bytes[i] == b' ' || bytes[i] == b'\t' || (skip_star && bytes[i] == b'*'))
    {
        i += 1;
    }
    while i < bytes.len() {
        if bytes[i] == b' ' {
            let start = i;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            // only count if surrounded by non-space (inter-word)
            let before_ok = start > opener
                && bytes[start - 1] != b' '
                && bytes[start - 1] != b'\t'
                && bytes[start - 1] != b'\n';
            let after_ok = i < bytes.len()
                && bytes[i] != b' '
                && bytes[i] != b'\t'
                && bytes[i] != b'\n'
                && bytes[i] != b'\r';
            let closer = bytes[i..].starts_with(b"*/")
                || bytes[i..].starts_with(b"-->")
                || bytes[i..].starts_with(b"--!>");
            if before_ok && after_ok && !closer {
                runs.push(SpaceRun {
                    start,
                    len: i - start,
                });
            }
            continue;
        }
        i += 1;
    }
    runs
}

fn rewrite_space_runs(
    comment: &str,
    reader: &mut BitReader,
    written: &mut usize,
    bit_len: usize,
) -> String {
    let runs = space_runs(comment);
    if runs.is_empty() {
        return comment.to_string();
    }
    let mut out = String::new();
    let mut last = 0usize;
    for run in runs {
        out.push_str(&comment[last..run.start]);
        let spaces = if *written < bit_len {
            let bit = reader.read_bit().unwrap_or(false);
            *written += 1;
            if bit { 2 } else { 1 }
        } else {
            1
        };
        for _ in 0..spaces {
            out.push(' ');
        }
        last = run.start + run.len;
    }
    out.push_str(&comment[last..]);
    out
}

fn normalize_spaces(comment: &str) -> String {
    let mut dummy = BitReader::new(&[]);
    let mut w = 0;
    // Force all bits as 0 by rewriting with empty reader after setting written=bit_len
    let runs = space_runs(comment);
    let mut out = String::new();
    let mut last = 0usize;
    for run in runs {
        out.push_str(&comment[last..run.start]);
        out.push(' ');
        last = run.start + run.len;
        let _ = (&mut dummy, &mut w);
    }
    out.push_str(&comment[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_space_runs_ignore_closer() {
        let runs = space_runs("<!-- alpha  bravo -->");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len, 2);
        let bang = space_runs("<!-- alpha  bravo --!>");
        assert_eq!(bang.len(), 1);
        let touching = space_runs("<!-- alpha -->");
        assert!(touching.is_empty(), "{touching:?}");
    }
}
