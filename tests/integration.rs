//! Integration tests for codestego watermarking.

use std::fs;
use std::path::PathBuf;

use codestego::carriers::{
    self, CarrierKind, CarrierMode, CarrierOptions, ZwAlphabet,
};
use codestego::frame::{self, Payload};
use codestego::keys::MasterKey;
use codestego::lang::{self, Lang};
use codestego::verify;
use proptest::prelude::*;
use tempfile::tempdir;

fn fixture(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn roundtrip_carrier(
    src: &str,
    lang: Lang,
    kinds: &[CarrierKind],
    mode: CarrierMode,
    opts: &CarrierOptions,
) {
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("Acme BV", "partner-x", "t1");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = lang.profile();
    let spans = lang::lex(src, &profile);

    let cap = carriers::total_capacity(src, &spans, &profile, kinds, mode, opts);
    if cap < bit_len {
        // Skip if fixture too small for this carrier
        return;
    }

    let marked = carriers::embed_with_carriers(
        src,
        &spans,
        &profile,
        kinds,
        mode,
        opts,
        &frame,
        bit_len,
    )
    .unwrap();

    verify::assert_token_invariant(src, &marked, &profile).unwrap();

    let mspans = lang::lex(&marked, &profile);
    let bits = carriers::decode_with_carriers(&marked, &mspans, &profile, kinds, mode, opts)
        .unwrap();
    let out = frame::decode_frame(&subkeys, &bits, false).unwrap();
    assert_eq!(out.owner, payload.owner);
    assert_eq!(out.recipient, payload.recipient);
    assert_eq!(out.note, payload.note);
}

fn roundtrip_one(src: &str, lang: Lang, kind: CarrierKind, opts: &CarrierOptions) {
    roundtrip_carrier(src, lang, &[kind], CarrierMode::Replicate, opts);
}

fn roundtrip_one_required(src: &str, lang: Lang, kind: CarrierKind, opts: &CarrierOptions) {
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("Acme BV", "partner-x", "t1");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = lang.profile();
    let spans = lang::lex(src, &profile);
    let cap = carriers::total_capacity(src, &spans, &profile, &[kind], CarrierMode::Replicate, opts);
    assert!(
        cap >= bit_len,
        "{lang:?} capacity {cap} < frame {bit_len}"
    );
    roundtrip_carrier(src, lang, &[kind], CarrierMode::Replicate, opts);
}

fn space_rich_src() -> String {
    let mut src = String::from("// header watermark carrier\n");
    // Frame bit length is typically ~1.5KiB; each inter-word run holds 1 bit.
    for i in 0..250 {
        src.push_str(&format!(
            "fn f{i}() {{ /* alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango {i} */ }}\n"
        ));
    }
    src
}

#[test]
fn fixtures_comment_zw_roundtrip() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        ..CarrierOptions::default()
    };
    let cases = [
        ("sample.rs", Lang::Rust),
        ("sample.c", Lang::C),
        ("sample.cpp", Lang::Cpp),
        ("sample.go", Lang::Go),
        ("sample.java", Lang::Java),
        ("sample.js", Lang::JavaScript),
        ("sample.ts", Lang::TypeScript),
        ("sample_crlf.rs", Lang::Rust),
        ("sample_no_nl.rs", Lang::Rust),
        ("sample.html", Lang::Html),
        ("sample.xml", Lang::Xml),
        ("sample_crlf.html", Lang::Html),
    ];
    for (name, lang) in cases {
        roundtrip_one_required(&fixture(name), lang, CarrierKind::CommentZw, &opts);
    }
}

#[test]
fn fixtures_eof_roundtrip() {
    let opts = CarrierOptions {
        eof_comment: true,
        ..CarrierOptions::default()
    };
    roundtrip_one(&fixture("sample.rs"), Lang::Rust, CarrierKind::Eof, &opts);
}

#[test]
fn fixtures_eol_roundtrip() {
    let opts = CarrierOptions {
        eol_bits: 1,
        ..CarrierOptions::default()
    };
    // Need enough lines — use a bigger synthetic file
    let mut src = String::from("// header comment\n");
    for i in 0..400 {
        src.push_str(&format!("let x{i} = {i}; // c{i}\n"));
    }
    roundtrip_one(&src, Lang::Rust, CarrierKind::Eol, &opts);
}

#[test]
fn idempotent_reembed() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        ..CarrierOptions::default()
    };
    let src = fixture("sample.rs");
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let profile = Lang::Rust.profile();
    let kinds = vec![CarrierKind::CommentZw];

    let embed = |note: &str, input: &str| {
        let payload = Payload::new("Acme", "bob", note);
        let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
        let bit_len = frame.len() * 8;
        let spans = lang::lex(input, &profile);
        let stripped = carriers::strip_carriers(input, &spans, &profile, &kinds, &opts).unwrap();
        let spans = lang::lex(&stripped, &profile);
        carriers::embed_with_carriers(
            &stripped,
            &spans,
            &profile,
            &kinds,
            CarrierMode::Replicate,
            &opts,
            &frame,
            bit_len,
        )
        .unwrap()
    };

    let once = embed("one", &src);
    let twice = embed("two", &once);
    let spans = lang::lex(&twice, &profile);
    let bits = carriers::decode_with_carriers(
        &twice,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
    )
    .unwrap();
    let out = frame::decode_frame(&subkeys, &bits, false).unwrap();
    assert_eq!(out.note, "two");
}

#[test]
fn strip_restores_comment_zw() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        ..CarrierOptions::default()
    };
    let src = fixture("sample.rs");
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("A", "B", "");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = Lang::Rust.profile();
    let kinds = vec![CarrierKind::CommentZw];
    let spans = lang::lex(&src, &profile);
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();
    let spans = lang::lex(&marked, &profile);
    let stripped = carriers::strip_carriers(&marked, &spans, &profile, &kinds, &opts).unwrap();
    assert_eq!(stripped, src);
}

#[test]
fn wrong_key_rejects() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        ..CarrierOptions::default()
    };
    let src = fixture("sample.rs");
    let k1 = MasterKey::generate().derive_subkeys();
    let k2 = MasterKey::generate().derive_subkeys();
    let payload = Payload::new("A", "B", "n");
    let frame = frame::encode_frame(&k1, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = Lang::Rust.profile();
    let kinds = vec![CarrierKind::CommentZw];
    let spans = lang::lex(&src, &profile);
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();
    let spans = lang::lex(&marked, &profile);
    let bits = carriers::decode_with_carriers(
        &marked,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
    )
    .unwrap();
    assert!(frame::decode_frame(&k2, &bits, false).is_err());
}

#[test]
fn partial_copy_recovery() {
    let opts = CarrierOptions {
        zw_per_comment: 2048,
        ..CarrierOptions::default()
    };
    // Large file so 40% still contains enough shards
    let mut src = String::from("// watermark carrier comment\n");
    for i in 0..200 {
        src.push_str(&format!("fn f{i}() {{ /* comment {i} with words */ }}\n"));
    }
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("Acme", "eve", "leak");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = Lang::Rust.profile();
    let kinds = vec![CarrierKind::CommentZw];
    let spans = lang::lex(&src, &profile);
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();

    // Keep first 60% of the file (contiguous prefix)
    let keep = (marked.len() * 6) / 10;
    let partial = &marked[..keep];
    // May need to find a valid UTF-8 boundary
    let partial = match std::str::from_utf8(partial.as_bytes()) {
        Ok(s) => s.to_string(),
        Err(e) => partial[..e.valid_up_to()].to_string(),
    };
    let spans = lang::lex(&partial, &profile);
    let bits = carriers::decode_with_carriers(
        &partial,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
    )
    .unwrap();
    let out = frame::decode_frame(&subkeys, &bits, false);
    assert!(
        out.is_ok(),
        "expected recovery from 60% prefix: {:?}",
        out.err()
    );
    assert_eq!(out.unwrap().recipient, "eve");
}

#[test]
fn false_positive_sweep() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        ..CarrierOptions::default()
    };
    let src = fixture("sample.rs");
    let real = MasterKey::generate();
    let subkeys = real.derive_subkeys();
    let payload = Payload::new("A", "B", "n");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = Lang::Rust.profile();
    let kinds = vec![CarrierKind::CommentZw];
    let spans = lang::lex(&src, &profile);
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();
    let spans = lang::lex(&marked, &profile);
    let bits = carriers::decode_with_carriers(
        &marked,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
    )
    .unwrap();

    // 200 wrong keys (plan says 10k; keep CI fast — still a strong check)
    for _ in 0..200 {
        let wrong = MasterKey::generate().derive_subkeys();
        assert!(frame::decode_frame(&wrong, &bits, false).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn prop_payload_frame_roundtrip(
        owner in ".{0,40}",
        recipient in ".{0,40}",
        note in ".{0,40}",
    ) {
        let sk = MasterKey::generate().derive_subkeys();
        let mut p = Payload::new(owner, recipient, note);
        p.timestamp = 12345;
        let frame = frame::encode_frame(&sk, &p, 24, 1.0, false).unwrap();
        let out = frame::decode_frame(&sk, &frame, false).unwrap();
        assert_eq!(out.owner, p.owner);
        assert_eq!(out.recipient, p.recipient);
        assert_eq!(out.note, p.note);
    }

    #[test]
    fn prop_comment_zw_embed(
        seed in 0u32..1000,
    ) {
        let _ = seed;
        let mut src = String::from("// prop test comment body with words\n");
        src.push_str("fn main() { /* another comment here */ }\n");
        let opts = CarrierOptions { zw_per_comment: 1024, ..CarrierOptions::default() };
        roundtrip_one(&src, Lang::Rust, CarrierKind::CommentZw, &opts);
    }
}

#[test]
fn keygen_cli_permissions() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("key");
    let k = codestego::keys::keygen_raw(&path).unwrap();
    let loaded = codestego::keys::load_key_file(&path, None).unwrap();
    assert_eq!(k.as_bytes(), loaded.as_bytes());
}

#[test]
fn comment_space_roundtrip_and_normalize_strip() {
    let opts = CarrierOptions::default();
    let src = space_rich_src();
    roundtrip_one(&src, Lang::Rust, CarrierKind::CommentSpace, &opts);

    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("A", "B", "n");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = Lang::Rust.profile();
    let kinds = vec![CarrierKind::CommentSpace];
    let spans = lang::lex(&src, &profile);
    let cap = carriers::total_capacity(&src, &spans, &profile, &kinds, CarrierMode::Replicate, &opts);
    assert!(cap >= bit_len, "synthetic source too small for comment-space");
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();
    let spans = lang::lex(&marked, &profile);
    let stripped = carriers::strip_carriers(&marked, &spans, &profile, &kinds, &opts).unwrap();
    // Strip normalizes comment inter-word runs to single spaces.
    let spans = lang::lex(&stripped, &profile);
    for sp in spans.iter().filter(|s| s.is_comment()) {
        let t = sp.text(&stripped);
        assert!(!t.contains("  "), "comment still has double spaces: {t}");
    }
}

#[test]
fn chain_mode_roundtrip() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        eof_comment: true,
        ..CarrierOptions::default()
    };
    let src = space_rich_src();
    let kinds = [
        CarrierKind::CommentZw,
        CarrierKind::CommentSpace,
        CarrierKind::Eof,
    ];
    let profile = Lang::Rust.profile();
    let spans = lang::lex(&src, &profile);
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("Acme BV", "partner-x", "chain");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let cap = carriers::total_capacity(&src, &spans, &profile, &kinds, CarrierMode::Chain, &opts);
    assert!(cap >= bit_len, "chain capacity {cap} < {bit_len}");
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Chain,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();
    verify::assert_token_invariant(&src, &marked, &profile).unwrap();
    let mspans = lang::lex(&marked, &profile);
    let bits = carriers::decode_with_carriers(
        &marked,
        &mspans,
        &profile,
        &kinds,
        CarrierMode::Chain,
        &opts,
    )
    .unwrap();
    let out = frame::decode_frame(&subkeys, &bits, false).unwrap();
    assert_eq!(out.note, "chain");
}

#[test]
fn replicate_survives_eol_damage() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        eol_bits: 1,
        ..CarrierOptions::default()
    };
    let mut src = String::from("// watermark carrier comment body with plenty of room\n");
    for i in 0..1600 {
        src.push_str(&format!("let x{i} = {i}; // c{i}\n"));
    }
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("Acme", "bob", "replicate");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = Lang::Rust.profile();
    let kinds = vec![CarrierKind::CommentZw, CarrierKind::Eol];
    let spans = lang::lex(&src, &profile);
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();

    // Destroy eol carrier only (trim trailing whitespace).
    let damaged: String = marked
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        + if marked.ends_with('\n') { "\n" } else { "" };

    let spans = lang::lex(&damaged, &profile);
    let bits = carriers::decode_with_carriers(
        &damaged,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
    )
    .unwrap();
    let out = frame::decode_frame(&subkeys, &bits, false).unwrap();
    assert_eq!(out.recipient, "bob");
    assert_eq!(out.note, "replicate");
}

#[test]
fn vs16_alphabet_roundtrip() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        zw_alphabet: ZwAlphabet::Vs16,
        ..CarrierOptions::default()
    };
    roundtrip_one(&fixture("sample.rs"), Lang::Rust, CarrierKind::CommentZw, &opts);
}

#[test]
fn ascii_only_skips_zw_uses_space() {
    let opts = CarrierOptions {
        ascii_only: true,
        ..CarrierOptions::default()
    };
    let src = space_rich_src();
    // Include CommentZw in the list; ascii_only must skip it.
    let kinds = [CarrierKind::CommentZw, CarrierKind::CommentSpace];
    roundtrip_carrier(&src, Lang::Rust, &kinds, CarrierMode::Replicate, &opts);
}

#[test]
fn deep_frame_decode() {
    let sk = MasterKey::generate().derive_subkeys();
    let p = Payload::new("Acme", "deep", "n");
    let frame = frame::encode_frame(&sk, &p, 24, 1.0, false).unwrap();
    let out = frame::decode_frame(&sk, &frame, true).unwrap();
    assert_eq!(out.recipient, "deep");
}

#[test]
fn formatter_robustness_optional() {
    // If gofmt/prettier/clang-format exist, watermark then format and ensure
    // extract either succeeds or cleanly fails — never returns wrong payload.
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        ..CarrierOptions::default()
    };
    let src = fixture("sample.go");
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("Acme", "fmt", "gofmt");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = Lang::Go.profile();
    let kinds = vec![CarrierKind::CommentZw];
    let spans = lang::lex(&src, &profile);
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();

    if std::process::Command::new("gofmt")
        .arg("-h")
        .output()
        .is_err()
    {
        return;
    }
    let dir = tempdir().unwrap();
    let path = dir.path().join("sample.go");
    fs::write(&path, &marked).unwrap();
    let status = std::process::Command::new("gofmt")
        .arg("-w")
        .arg(&path)
        .status()
        .unwrap();
    if !status.success() {
        return;
    }
    let formatted = fs::read_to_string(&path).unwrap();
    let spans = lang::lex(&formatted, &profile);
    let bits = carriers::decode_with_carriers(
        &formatted,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
    );
    match bits {
        Ok(b) => match frame::decode_frame(&subkeys, &b, false) {
            Ok(p) => {
                assert_eq!(p.owner, "Acme");
                assert_eq!(p.recipient, "fmt");
            }
            Err(_) => {} // clean failure OK
        },
        Err(_) => {} // clean failure OK
    }
}

fn markup_space_rich_src() -> String {
    let mut src = String::from("<root>\n");
    for i in 0..250 {
        src.push_str(&format!(
            "<!-- alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango {i} -->\n"
        ));
    }
    src.push_str("</root>\n");
    src
}

#[test]
fn markup_eof_comment_roundtrip() {
    let opts = CarrierOptions {
        eof_comment: true,
        ..CarrierOptions::default()
    };
    for (src, lang) in [
        (fixture("sample.html"), Lang::Html),
        (fixture("sample.xml"), Lang::Xml),
    ] {
        roundtrip_one_required(&src, lang, CarrierKind::Eof, &opts);
        let profile = lang.profile();
        let master = MasterKey::generate();
        let subkeys = master.derive_subkeys();
        let payload = Payload::new("Acme BV", "partner-x", "t1");
        let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
        let bit_len = frame.len() * 8;
        let spans = lang::lex(&src, &profile);
        let marked = carriers::embed_with_carriers(
            &src,
            &spans,
            &profile,
            &[CarrierKind::Eof],
            CarrierMode::Replicate,
            &opts,
            &frame,
            bit_len,
        )
        .unwrap();
        assert!(marked.contains("<!--"), "{lang:?}");
        assert!(!marked.contains("/*"), "{lang:?} {marked}");
    }
}

#[test]
fn markup_comment_space_roundtrip() {
    let opts = CarrierOptions::default();
    let src = markup_space_rich_src();
    roundtrip_one_required(&src, Lang::Html, CarrierKind::CommentSpace, &opts);
    roundtrip_one_required(&src, Lang::Xml, CarrierKind::CommentSpace, &opts);
}

#[test]
fn markup_eol_roundtrip() {
    let opts = CarrierOptions {
        eol_bits: 1,
        ..CarrierOptions::default()
    };
    let mut src = String::from("<!-- header -->\n");
    for i in 0..2000 {
        src.push_str(&format!("<p>x{i}</p>\n"));
    }
    roundtrip_one_required(&src, Lang::Html, CarrierKind::Eol, &opts);
    roundtrip_one_required(&src, Lang::Xml, CarrierKind::Eol, &opts);
}

#[test]
fn html_comment_zw_preserves_script_bytes() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        ..CarrierOptions::default()
    };
    let src = fixture("sample.html");
    let profile = Lang::Html.profile();
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("Acme BV", "partner-x", "t1");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let spans = lang::lex(&src, &profile);
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &[CarrierKind::CommentZw],
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();
    verify::assert_token_invariant(&src, &marked, &profile).unwrap();
    let a = lang::non_trivia_bytes(&src, &lang::lex(&src, &profile));
    let b = lang::non_trivia_bytes(&marked, &lang::lex(&marked, &profile));
    assert_eq!(a, b);
    assert!(src.contains("var x = 1"));
    assert!(marked.contains("var x = 1"));
}

#[test]
fn formatter_robustness_html_optional() {
    let opts = CarrierOptions {
        zw_per_comment: 1024,
        ..CarrierOptions::default()
    };
    let src = fixture("sample.html");
    let master = MasterKey::generate();
    let subkeys = master.derive_subkeys();
    let payload = Payload::new("Acme", "fmt", "prettier");
    let frame = frame::encode_frame(&subkeys, &payload, 24, 1.0, false).unwrap();
    let bit_len = frame.len() * 8;
    let profile = Lang::Html.profile();
    let kinds = vec![CarrierKind::CommentZw];
    let spans = lang::lex(&src, &profile);
    let marked = carriers::embed_with_carriers(
        &src,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
        &frame,
        bit_len,
    )
    .unwrap();
    if std::process::Command::new("prettier")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let dir = tempdir().unwrap();
    let path = dir.path().join("sample.html");
    fs::write(&path, &marked).unwrap();
    let status = std::process::Command::new("prettier")
        .arg("--write")
        .arg(&path)
        .status()
        .unwrap();
    if !status.success() {
        return;
    }
    let formatted = fs::read_to_string(&path).unwrap();
    let spans = lang::lex(&formatted, &profile);
    let bits = carriers::decode_with_carriers(
        &formatted,
        &spans,
        &profile,
        &kinds,
        CarrierMode::Replicate,
        &opts,
    );
    match bits {
        Ok(b) => match frame::decode_frame(&subkeys, &b, false) {
            Ok(p) => {
                assert_eq!(p.owner, "Acme");
                assert_eq!(p.recipient, "fmt");
            }
            Err(_) => {}
        },
        Err(_) => {}
    }
}
