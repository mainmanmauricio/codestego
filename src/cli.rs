//! clap CLI definitions and command dispatch.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::carriers::{
    self, CarrierKind, CarrierMode, CarrierOptions, ZwAlphabet,
};
use crate::frame::{self, Payload, DEFAULT_SHARD_SIZE};
use crate::keys::{self, MasterKey};
use crate::lang::{self, Lang};
use crate::{doctor, journal, verify};

#[derive(Parser, Debug)]
#[command(
    name = "codestego",
    version,
    long_version = concat!(
        env!("CARGO_PKG_VERSION"),
        "\nCopyright (C) 2026 Maurice Gittens. Licensed under the GNU General Public License, version 2. See LICENSE."
    ),
    about = "Keyed steganographic watermarking for source code",
    after_help = "Copyright (C) 2026 Maurice Gittens. Licensed under the GNU General Public License, version 2. See LICENSE."
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate a master key file (mode 0600)
    Keygen {
        #[arg(long, default_value_os_t = keys::default_key_path())]
        out: PathBuf,
        /// Derive key from passphrase (Argon2id) instead of storing raw bytes
        #[arg(long)]
        passphrase: bool,
    },
    /// Embed a watermark into source files
    Embed {
        #[arg(long)]
        key: Option<PathBuf>,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        owner: String,
        #[arg(long, default_value = "")]
        recipient: String,
        #[arg(long, default_value = "")]
        note: String,
        #[arg(long, default_value = "comment-zw")]
        carriers: String,
        #[arg(long, default_value = "replicate")]
        carrier_mode: String,
        #[arg(long, default_value_t = 1.0)]
        parity: f64,
        #[arg(long, default_value_t = DEFAULT_SHARD_SIZE)]
        shard_size: u8,
        #[arg(long)]
        ascii_only: bool,
        #[arg(long, default_value = "zw4")]
        zw_alphabet: String,
        #[arg(long, default_value_t = 1024)]
        zw_per_comment: usize,
        #[arg(long, default_value_t = 1)]
        eol_bits: u8,
        #[arg(long, default_value_t = 0)]
        eof_lines: usize,
        #[arg(long)]
        eof_comment: bool,
        #[arg(long, default_value = "auto")]
        lang: String,
        #[arg(long)]
        in_place: bool,
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        journal: Option<PathBuf>,
        #[arg(long)]
        compile_check: Option<String>,
        /// Derive AEAD nonce from key+payload (requires --timestamp)
        #[arg(long)]
        deterministic: bool,
        /// Unix timestamp for the payload (required with --deterministic)
        #[arg(long)]
        timestamp: Option<u32>,
        paths: Vec<PathBuf>,
    },
    /// Extract a watermark from source files
    Extract {
        #[arg(long)]
        key: Option<PathBuf>,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long, default_value = "auto")]
        carriers: String,
        #[arg(long)]
        deep: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "auto")]
        lang: String,
        #[arg(long, default_value = "replicate")]
        carrier_mode: String,
        #[arg(long)]
        ascii_only: bool,
        #[arg(long, default_value = "zw4")]
        zw_alphabet: String,
        #[arg(long, default_value_t = 1)]
        eol_bits: u8,
        #[arg(long)]
        eof_comment: bool,
        paths: Vec<PathBuf>,
    },
    /// Recursively scan for watermarks (exit 0 = found)
    Scan {
        #[arg(long)]
        key: Option<PathBuf>,
        #[arg(long)]
        passphrase: Option<String>,
        #[arg(long)]
        recursive: bool,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        deep: bool,
        paths: Vec<PathBuf>,
    },
    /// Report carrier capacity for files
    Capacity {
        #[arg(long, default_value = "comment-zw,comment-space,eol,eof")]
        carriers: String,
        #[arg(long, default_value = "replicate")]
        carrier_mode: String,
        #[arg(long)]
        ascii_only: bool,
        #[arg(long, default_value = "zw4")]
        zw_alphabet: String,
        #[arg(long, default_value_t = 1024)]
        zw_per_comment: usize,
        #[arg(long, default_value_t = 1)]
        eol_bits: u8,
        #[arg(long)]
        eof_comment: bool,
        #[arg(long, default_value = "auto")]
        lang: String,
        paths: Vec<PathBuf>,
    },
    /// Strip watermark carriers from files
    Strip {
        #[arg(long, default_value = "all")]
        carriers: String,
        #[arg(long)]
        in_place: bool,
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "auto")]
        lang: String,
        #[arg(long)]
        ascii_only: bool,
        #[arg(long)]
        eof_comment: bool,
        paths: Vec<PathBuf>,
    },
    /// Warn about formatters that destroy carriers
    Doctor {
        #[arg(long, default_value = "comment-zw,eol,comment-space,eof")]
        carriers: String,
        paths: Vec<PathBuf>,
    },
}

pub fn run(cli: Cli) -> Result<i32> {
    match cli.cmd {
        Command::Keygen { out, passphrase } => cmd_keygen(&out, passphrase),
        Command::Embed {
            key,
            passphrase,
            owner,
            recipient,
            note,
            carriers,
            carrier_mode,
            parity,
            shard_size,
            ascii_only,
            zw_alphabet,
            zw_per_comment,
            eol_bits,
            eof_lines,
            eof_comment,
            lang,
            in_place,
            output,
            dry_run,
            journal,
            compile_check,
            deterministic,
            timestamp,
            paths,
        } => cmd_embed(EmbedArgs {
            key,
            passphrase,
            owner,
            recipient,
            note,
            carriers,
            carrier_mode,
            parity,
            shard_size,
            ascii_only,
            zw_alphabet,
            zw_per_comment,
            eol_bits,
            eof_lines,
            eof_comment,
            lang,
            in_place,
            output,
            dry_run,
            journal,
            compile_check,
            deterministic,
            timestamp,
            paths,
        }),
        Command::Extract {
            key,
            passphrase,
            carriers,
            deep,
            json,
            lang,
            carrier_mode,
            ascii_only,
            zw_alphabet,
            eol_bits,
            eof_comment,
            paths,
        } => cmd_extract(ExtractArgs {
            key,
            passphrase,
            carriers,
            deep,
            json,
            lang,
            carrier_mode,
            ascii_only,
            zw_alphabet,
            eol_bits,
            eof_comment,
            paths,
        }),
        Command::Scan {
            key,
            passphrase,
            recursive,
            json,
            deep,
            paths,
        } => cmd_scan(key, passphrase, recursive, json, deep, paths),
        Command::Capacity {
            carriers,
            carrier_mode,
            ascii_only,
            zw_alphabet,
            zw_per_comment,
            eol_bits,
            eof_comment,
            lang,
            paths,
        } => cmd_capacity(
            &carriers,
            &carrier_mode,
            ascii_only,
            &zw_alphabet,
            zw_per_comment,
            eol_bits,
            eof_comment,
            &lang,
            &paths,
        ),
        Command::Strip {
            carriers,
            in_place,
            output,
            lang,
            ascii_only,
            eof_comment,
            paths,
        } => cmd_strip(&carriers, in_place, output, &lang, ascii_only, eof_comment, &paths),
        Command::Doctor { carriers, paths } => cmd_doctor(&carriers, &paths),
    }
}

fn cmd_keygen(out: &Path, passphrase: bool) -> Result<i32> {
    if passphrase {
        eprint!("Passphrase: ");
        let pass = read_passphrase()?;
        keys::keygen_passphrase(out, &pass)?;
        eprintln!("Wrote passphrase-protected key to {}", out.display());
    } else {
        keys::keygen_raw(out)?;
        eprintln!("Wrote raw key to {}", out.display());
    }
    Ok(0)
}

fn read_passphrase() -> Result<String> {
    // Simple stdin read (no tty echo control for portability in v1)
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

struct EmbedArgs {
    key: Option<PathBuf>,
    passphrase: Option<String>,
    owner: String,
    recipient: String,
    note: String,
    carriers: String,
    carrier_mode: String,
    parity: f64,
    shard_size: u8,
    ascii_only: bool,
    zw_alphabet: String,
    zw_per_comment: usize,
    eol_bits: u8,
    eof_lines: usize,
    eof_comment: bool,
    lang: String,
    in_place: bool,
    output: Option<PathBuf>,
    dry_run: bool,
    journal: Option<PathBuf>,
    compile_check: Option<String>,
    deterministic: bool,
    timestamp: Option<u32>,
    paths: Vec<PathBuf>,
}

fn cmd_embed(a: EmbedArgs) -> Result<i32> {
    if a.paths.is_empty() {
        bail!("no paths given");
    }
    if a.deterministic && a.timestamp.is_none() {
        bail!("--deterministic requires --timestamp <unix-seconds>");
    }
    let master = keys::resolve_key(a.key.as_deref(), a.passphrase.as_deref())?;
    let subkeys = master.derive_subkeys();
    let kinds = parse_carriers(&a.carriers)?;
    let mode = CarrierMode::parse(&a.carrier_mode)?;
    let opts = CarrierOptions {
        ascii_only: a.ascii_only,
        zw_alphabet: ZwAlphabet::parse(&a.zw_alphabet)?,
        zw_per_comment: a.zw_per_comment,
        eol_bits: a.eol_bits,
        eof_lines: a.eof_lines,
        eof_comment: a.eof_comment,
    };
    let payload = if let Some(ts) = a.timestamp {
        Payload::with_timestamp(&a.owner, &a.recipient, &a.note, ts)
    } else {
        Payload::new(&a.owner, &a.recipient, &a.note)
    };
    let frame = frame::encode_frame(
        &subkeys,
        &payload,
        a.shard_size,
        a.parity,
        a.deterministic,
    )?;
    let bit_len = frame.len() * 8;

    for path in &a.paths {
        let src = read_source(path)?;
        let lang = lang::detect_lang(Some(&a.lang), path)
            .ok_or_else(|| anyhow::anyhow!("cannot detect language for {}", path.display()))?;
        let profile = lang.profile();
        let spans = lang::lex(&src, &profile);

        // Strip existing marks first for idempotent re-embed
        let stripped = carriers::strip_carriers(&src, &spans, &profile, &kinds, &opts)?;
        let spans = lang::lex(&stripped, &profile);

        let cap = carriers::total_capacity(&stripped, &spans, &profile, &kinds, mode, &opts);
        if cap < bit_len {
            bail!(
                "{}: capacity {cap} bits < frame {bit_len} bits (add comments, enable eof, or lower parity)",
                path.display()
            );
        }

        let watermarked = carriers::embed_with_carriers(
            &stripped, &spans, &profile, &kinds, mode, &opts, &frame, bit_len,
        )?;

        verify::assert_token_invariant(&stripped, &watermarked, &profile)?;

        // Round-trip extract check
        let wspans = lang::lex(&watermarked, &profile);
        let bits = carriers::decode_with_carriers(
            &watermarked, &wspans, &profile, &kinds, mode, &opts,
        )?;
        let recovered = frame::decode_frame(&subkeys, &bits, false)
            .context("post-embed extract verification failed")?;
        if recovered.owner != payload.owner
            || recovered.recipient != payload.recipient
            || recovered.note != payload.note
        {
            bail!("post-embed verification: payload mismatch");
        }

        if a.dry_run {
            println!(
                "{}: ok (dry-run) frame_bits={bit_len} capacity={cap} lang={}",
                path.display(),
                lang.name()
            );
            continue;
        }

        let out_path = if a.in_place {
            path.clone()
        } else if let Some(ref o) = a.output {
            if a.paths.len() > 1 {
                bail!("-o/--output only valid with a single input path");
            }
            o.clone()
        } else {
            bail!("specify --in-place or -o/--output");
        };

        // Write to temp then rename for safety
        let tmp = out_path.with_extension(format!(
            "{}codestego.tmp",
            out_path.extension().and_then(|e| e.to_str()).unwrap_or("")
        ));
        std::fs::write(&tmp, &watermarked)
            .with_context(|| format!("write {}", tmp.display()))?;

        if let Some(ref cc) = a.compile_check {
            if let Err(e) = verify::compile_check(cc, &tmp) {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }

        std::fs::rename(&tmp, &out_path)
            .with_context(|| format!("rename to {}", out_path.display()))?;

        if let Some(ref jp) = a.journal {
            journal::append(jp, &master.journal_key(), path, &src, &watermarked, &payload)?;
        }

        println!(
            "{}: embedded ({} bits, {})",
            out_path.display(),
            bit_len,
            lang.name()
        );
    }
    Ok(0)
}

struct ExtractArgs {
    key: Option<PathBuf>,
    passphrase: Option<String>,
    carriers: String,
    deep: bool,
    json: bool,
    lang: String,
    carrier_mode: String,
    ascii_only: bool,
    zw_alphabet: String,
    eol_bits: u8,
    eof_comment: bool,
    paths: Vec<PathBuf>,
}

fn cmd_extract(a: ExtractArgs) -> Result<i32> {
    if a.paths.is_empty() {
        bail!("no paths given");
    }
    let master = keys::resolve_key(a.key.as_deref(), a.passphrase.as_deref())?;
    let subkeys = master.derive_subkeys();
    let kinds = if a.carriers == "auto" {
        CarrierKind::all()
    } else {
        parse_carriers(&a.carriers)?
    };
    let mode = CarrierMode::parse(&a.carrier_mode)?;
    let opts = CarrierOptions {
        ascii_only: a.ascii_only,
        zw_alphabet: ZwAlphabet::parse(&a.zw_alphabet)?,
        zw_per_comment: 64,
        eol_bits: a.eol_bits,
        eof_lines: 0,
        eof_comment: a.eof_comment,
    };

    let mut any = false;
    for path in &a.paths {
        match extract_one(path, &a.lang, &subkeys, &kinds, mode, &opts, a.deep) {
            Ok(p) => {
                any = true;
                if a.json {
                    println!("{}", serde_json::to_string(&ExtractJson {
                        path: path.display().to_string(),
                        owner: p.owner,
                        recipient: p.recipient,
                        note: p.note,
                        timestamp: p.timestamp,
                    })?);
                } else {
                    println!(
                        "{}: owner={:?} recipient={:?} note={:?} ts={}",
                        path.display(),
                        p.owner,
                        p.recipient,
                        p.note,
                        p.timestamp
                    );
                }
            }
            Err(e) => {
                if a.json {
                    eprintln!("{}: {e}", path.display());
                } else {
                    eprintln!("{}: not found ({e})", path.display());
                }
            }
        }
    }
    Ok(if any { 0 } else { 1 })
}

#[derive(serde::Serialize)]
struct ExtractJson {
    path: String,
    owner: String,
    recipient: String,
    note: String,
    timestamp: u32,
}

fn extract_one(
    path: &Path,
    lang_opt: &str,
    subkeys: &keys::SubKeys,
    kinds: &[CarrierKind],
    mode: CarrierMode,
    opts: &CarrierOptions,
    deep: bool,
) -> Result<Payload> {
    let src = read_source(path)?;
    let lang = lang::detect_lang(Some(lang_opt), path)
        .ok_or_else(|| anyhow::anyhow!("cannot detect language"))?;
    let profile = lang.profile();
    let spans = lang::lex(&src, &profile);
    let bits = carriers::decode_with_carriers(&src, &spans, &profile, kinds, mode, opts)?;
    frame::decode_frame(subkeys, &bits, deep)
}

fn cmd_scan(
    key: Option<PathBuf>,
    passphrase: Option<String>,
    recursive: bool,
    json: bool,
    deep: bool,
    paths: Vec<PathBuf>,
) -> Result<i32> {
    if paths.is_empty() {
        bail!("no paths given");
    }
    let master = keys::resolve_key(key.as_deref(), passphrase.as_deref())?;
    let subkeys = master.derive_subkeys();
    let kinds = CarrierKind::all();
    let mode = CarrierMode::Replicate;
    let opts = CarrierOptions::default();

    let files = collect_files(&paths, recursive)?;
    let mut found = 0usize;
    for path in files {
        match extract_one(&path, "auto", &subkeys, &kinds, mode, &opts, deep) {
            Ok(p) => {
                found += 1;
                if json {
                    println!("{}", serde_json::to_string(&ExtractJson {
                        path: path.display().to_string(),
                        owner: p.owner,
                        recipient: p.recipient,
                        note: p.note,
                        timestamp: p.timestamp,
                    })?);
                } else {
                    println!(
                        "FOUND {} owner={:?} recipient={:?}",
                        path.display(),
                        p.owner,
                        p.recipient
                    );
                }
            }
            Err(_) => {}
        }
    }
    if !json {
        eprintln!("scanned; {found} watermark(s) found");
    }
    Ok(if found > 0 { 0 } else { 1 })
}

fn cmd_capacity(
    carriers_s: &str,
    mode_s: &str,
    ascii_only: bool,
    zw_alphabet: &str,
    zw_per_comment: usize,
    eol_bits: u8,
    eof_comment: bool,
    lang_s: &str,
    paths: &[PathBuf],
) -> Result<i32> {
    let kinds = parse_carriers(carriers_s)?;
    let mode = CarrierMode::parse(mode_s)?;
    let opts = CarrierOptions {
        ascii_only,
        zw_alphabet: ZwAlphabet::parse(zw_alphabet)?,
        zw_per_comment,
        eol_bits,
        eof_lines: 0,
        eof_comment,
    };
    for path in paths {
        let src = read_source(path)?;
        let lang = lang::detect_lang(Some(lang_s), path)
            .ok_or_else(|| anyhow::anyhow!("cannot detect language for {}", path.display()))?;
        let profile = lang.profile();
        let spans = lang::lex(&src, &profile);
        let total = carriers::total_capacity(&src, &spans, &profile, &kinds, mode, &opts);
        print!("{}: total={total} bits", path.display());
        for k in &kinds {
            let c = carriers::make_carrier(*k, &opts);
            let cap = c.capacity(&src, &spans, &profile);
            print!(" {}={}", k.name(), cap);
        }
        println!();
    }
    Ok(0)
}

fn cmd_strip(
    carriers_s: &str,
    in_place: bool,
    output: Option<PathBuf>,
    lang_s: &str,
    ascii_only: bool,
    eof_comment: bool,
    paths: &[PathBuf],
) -> Result<i32> {
    let kinds = if carriers_s == "all" {
        CarrierKind::all()
    } else {
        parse_carriers(carriers_s)?
    };
    let opts = CarrierOptions {
        ascii_only,
        eof_comment,
        ..CarrierOptions::default()
    };
    for path in paths {
        let src = read_source(path)?;
        let lang = lang::detect_lang(Some(lang_s), path)
            .ok_or_else(|| anyhow::anyhow!("cannot detect language"))?;
        let profile = lang.profile();
        let spans = lang::lex(&src, &profile);
        let out = carriers::strip_carriers(&src, &spans, &profile, &kinds, &opts)?;
        let out_path = if in_place {
            path.clone()
        } else if let Some(ref o) = output {
            o.clone()
        } else {
            bail!("specify --in-place or -o");
        };
        std::fs::write(&out_path, out)?;
        println!("{}: stripped", out_path.display());
    }
    Ok(0)
}

fn cmd_doctor(carriers_s: &str, paths: &[PathBuf]) -> Result<i32> {
    let kinds = parse_carriers(carriers_s)?;
    let roots: Vec<&Path> = if paths.is_empty() {
        vec![Path::new(".")]
    } else {
        paths.iter().map(|p| p.as_path()).collect()
    };
    let warnings = doctor::inspect(&roots, &kinds)?;
    if warnings.is_empty() {
        println!("No formatter conflicts detected for selected carriers.");
        return Ok(0);
    }
    for w in &warnings {
        println!("{}: {} (affects: {})", w.path, w.message, w.affects.join(", "));
    }
    Ok(0)
}

fn parse_carriers(s: &str) -> Result<Vec<CarrierKind>> {
    if s == "all" {
        return Ok(CarrierKind::all());
    }
    CarrierKind::parse_list(s)
}

fn read_source(path: &Path) -> Result<String> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    if lang::is_binary_plist(path, &src) {
        bail!(
            "{}: binary plist is not supported; use an XML text plist",
            path.display()
        );
    }
    Ok(src)
}

fn collect_files(paths: &[PathBuf], recursive: bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_file() {
            if Lang::from_ext(p).is_some() {
                out.push(p.clone());
            }
            continue;
        }
        if p.is_dir() {
            if !recursive {
                // non-recursive: immediate children only
                for entry in std::fs::read_dir(p)? {
                    let entry = entry?;
                    let fp = entry.path();
                    if fp.is_file() && Lang::from_ext(&fp).is_some() {
                        out.push(fp);
                    }
                }
            } else {
                let walker = ignore::WalkBuilder::new(p)
                    .hidden(false)
                    .git_ignore(true)
                    .build();
                for dent in walker {
                    let dent = dent.map_err(|e| anyhow::anyhow!("walk: {e}"))?;
                    let fp = dent.path().to_path_buf();
                    if fp.is_file() && Lang::from_ext(&fp).is_some() {
                        out.push(fp);
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

// Silence unused import if MasterKey only used via resolve
#[allow(dead_code)]
fn _mk(_: MasterKey) {}
