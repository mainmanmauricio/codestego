//! Carrier channels for embedding bitstreams into source trivia.

mod comment_space;
mod comment_zw;
mod eof;
mod eol;

pub use comment_space::CommentSpaceCarrier;
pub use comment_zw::{CommentZwCarrier, ZwAlphabet};
pub use eof::EofCarrier;
pub use eol::EolCarrier;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::lang::{LangProfile, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CarrierKind {
    CommentZw,
    CommentSpace,
    Eol,
    Eof,
}

impl CarrierKind {
    pub fn parse_list(s: &str) -> Result<Vec<Self>> {
        let mut out = Vec::new();
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            out.push(Self::parse(part)?);
        }
        if out.is_empty() {
            bail!("no carriers specified");
        }
        Ok(out)
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "comment-zw" => Ok(Self::CommentZw),
            "comment-space" => Ok(Self::CommentSpace),
            "eol" => Ok(Self::Eol),
            "eof" => Ok(Self::Eof),
            "all" => bail!("use parse_all for 'all'"),
            other => bail!("unknown carrier: {other}"),
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::CommentZw,
            Self::CommentSpace,
            Self::Eol,
            Self::Eof,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::CommentZw => "comment-zw",
            Self::CommentSpace => "comment-space",
            Self::Eol => "eol",
            Self::Eof => "eof",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierMode {
    /// Full independent copy of the bitstream in each carrier.
    Replicate,
    /// Concatenate capacity across carriers.
    Chain,
}

impl CarrierMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "replicate" => Ok(Self::Replicate),
            "chain" => Ok(Self::Chain),
            other => bail!("unknown carrier mode: {other}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CarrierOptions {
    pub ascii_only: bool,
    pub zw_alphabet: ZwAlphabet,
    pub zw_per_comment: usize,
    pub eol_bits: u8,
    pub eof_lines: usize,
    pub eof_comment: bool,
}

impl Default for CarrierOptions {
    fn default() -> Self {
        Self {
            ascii_only: false,
            zw_alphabet: ZwAlphabet::Zw4,
            zw_per_comment: 1024,
            eol_bits: 1,
            eof_lines: 0, // auto-sized when encoding
            eof_comment: false,
        }
    }
}

pub trait Carrier {
    fn kind(&self) -> CarrierKind;
    fn capacity(&self, src: &str, spans: &[Span], profile: &LangProfile) -> usize;
    fn encode(
        &self,
        src: &str,
        spans: &[Span],
        profile: &LangProfile,
        bits: &[u8],
        bit_len: usize,
    ) -> Result<String>;
    fn decode(&self, src: &str, spans: &[Span], profile: &LangProfile) -> Result<Vec<u8>>;
    fn strip(&self, src: &str, spans: &[Span], profile: &LangProfile) -> Result<String>;
}

pub fn make_carrier(kind: CarrierKind, opts: &CarrierOptions) -> Box<dyn Carrier> {
    match kind {
        CarrierKind::CommentZw => Box::new(CommentZwCarrier {
            alphabet: opts.zw_alphabet,
            max_per_comment: opts.zw_per_comment,
            disabled: opts.ascii_only,
        }),
        CarrierKind::CommentSpace => Box::new(CommentSpaceCarrier),
        CarrierKind::Eol => Box::new(EolCarrier {
            bits_per_line: opts.eol_bits,
        }),
        CarrierKind::Eof => Box::new(EofCarrier {
            lines: opts.eof_lines,
            as_comment: opts.eof_comment,
            alphabet: opts.zw_alphabet,
            ascii_only: opts.ascii_only,
        }),
    }
}

/// Embed bits using the selected carriers and mode.
pub fn embed_with_carriers(
    src: &str,
    _spans: &[Span],
    profile: &LangProfile,
    kinds: &[CarrierKind],
    mode: CarrierMode,
    opts: &CarrierOptions,
    bits: &[u8],
    bit_len: usize,
) -> Result<String> {
    match mode {
        CarrierMode::Replicate => {
            let mut current = src.to_string();
            for kind in kinds {
                if *kind == CarrierKind::CommentZw && opts.ascii_only {
                    continue;
                }
                let carrier = make_carrier(*kind, opts);
                let spans = crate::lang::lex(&current, profile);
                let cap = carrier.capacity(&current, &spans, profile);
                if cap < bit_len {
                    bail!(
                        "carrier {} capacity {cap} bits < needed {bit_len}",
                        kind.name()
                    );
                }
                current = carrier.encode(&current, &spans, profile, bits, bit_len)?;
            }
            Ok(current)
        }
        CarrierMode::Chain => {
            // Split bit_len across carriers by capacity order
            let mut remaining_bits = bits.to_vec();
            let mut remaining_len = bit_len;
            let mut current = src.to_string();
            for kind in kinds {
                if remaining_len == 0 {
                    break;
                }
                if *kind == CarrierKind::CommentZw && opts.ascii_only {
                    continue;
                }
                let carrier = make_carrier(*kind, opts);
                let spans = crate::lang::lex(&current, profile);
                let cap = carrier.capacity(&current, &spans, profile);
                if cap == 0 {
                    continue;
                }
                let take = cap.min(remaining_len);
                let take_bytes = (take + 7) / 8;
                let chunk: Vec<u8> = remaining_bits.iter().take(take_bytes).copied().collect();
                current = carrier.encode(&current, &spans, profile, &chunk, take)?;
                // Shift remaining
                let mut r = crate::bits::BitReader::new(&remaining_bits);
                r.skip_bits(take);
                let left = remaining_len - take;
                let mut w = crate::bits::BitWriter::new();
                for _ in 0..left {
                    if let Some(b) = r.read_bit() {
                        w.write_bit(b);
                    }
                }
                remaining_bits = w.finish();
                remaining_len = left;
            }
            if remaining_len > 0 {
                bail!("insufficient chained capacity; {remaining_len} bits left");
            }
            Ok(current)
        }
    }
}

/// Try decode from carriers (replicate: any success; chain: concatenate).
pub fn decode_with_carriers(
    src: &str,
    spans: &[Span],
    profile: &LangProfile,
    kinds: &[CarrierKind],
    mode: CarrierMode,
    opts: &CarrierOptions,
) -> Result<Vec<u8>> {
    match mode {
        CarrierMode::Replicate => {
            let mut errs = Vec::new();
            for kind in kinds {
                if *kind == CarrierKind::CommentZw && opts.ascii_only {
                    continue;
                }
                let carrier = make_carrier(*kind, opts);
                match carrier.decode(src, spans, profile) {
                    Ok(bits) if !bits.is_empty() => return Ok(bits),
                    Ok(_) => errs.push(format!("{}: empty", kind.name())),
                    Err(e) => errs.push(format!("{}: {e}", kind.name())),
                }
            }
            bail!("no carrier decoded: {}", errs.join("; "));
        }
        CarrierMode::Chain => {
            let mut w = crate::bits::BitWriter::new();
            for kind in kinds {
                if *kind == CarrierKind::CommentZw && opts.ascii_only {
                    continue;
                }
                let carrier = make_carrier(*kind, opts);
                let bits = carrier.decode(src, spans, profile)?;
                let mut r = crate::bits::BitReader::new(&bits);
                // For chain we don't know exact bit lengths per carrier at decode;
                // take all available bits from each.
                while let Some(b) = r.read_bit() {
                    w.write_bit(b);
                }
            }
            Ok(w.finish())
        }
    }
}

pub fn strip_carriers(
    src: &str,
    _spans: &[Span],
    profile: &LangProfile,
    kinds: &[CarrierKind],
    opts: &CarrierOptions,
) -> Result<String> {
    let mut current = src.to_string();
    for kind in kinds {
        let carrier = make_carrier(*kind, opts);
        let spans = crate::lang::lex(&current, profile);
        current = carrier.strip(&current, &spans, profile)?;
    }
    Ok(current)
}

pub fn total_capacity(
    src: &str,
    spans: &[Span],
    profile: &LangProfile,
    kinds: &[CarrierKind],
    mode: CarrierMode,
    opts: &CarrierOptions,
) -> usize {
    match mode {
        CarrierMode::Replicate => kinds
            .iter()
            .filter(|k| !(**k == CarrierKind::CommentZw && opts.ascii_only))
            .map(|k| make_carrier(*k, opts).capacity(src, spans, profile))
            .min()
            .unwrap_or(0),
        CarrierMode::Chain => kinds
            .iter()
            .filter(|k| !(**k == CarrierKind::CommentZw && opts.ascii_only))
            .map(|k| make_carrier(*k, opts).capacity(src, spans, profile))
            .sum(),
    }
}
