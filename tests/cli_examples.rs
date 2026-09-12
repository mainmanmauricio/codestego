//! CLI black-box examples mirroring the user guide / README workflows.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codestego"))
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn codestego: {e}"))
}

fn fixture_rs(dir: &Path) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.rs");
    let dest = dir.join("sample.rs");
    fs::copy(&src, &dest).unwrap();
    dest
}

#[test]
fn quick_start_workflow() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    let file = fixture_rs(dir.path());

    let out = run(&["keygen", "--out", key.to_str().unwrap()]);
    assert!(out.status.success(), "keygen: {}", String::from_utf8_lossy(&out.stderr));

    let out = run(&[
        "capacity",
        "--carriers",
        "comment-zw",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "capacity: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!out.stdout.is_empty());

    let out = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme BV",
        "--recipient",
        "partner-x",
        "--note",
        "license-2026-03",
        "--in-place",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "embed: {}", String::from_utf8_lossy(&out.stderr));

    let out = run(&[
        "extract",
        "--key",
        key.to_str().unwrap(),
        "--json",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "extract: {}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["owner"], "Acme BV");
    assert_eq!(json["recipient"], "partner-x");
    assert_eq!(json["note"], "license-2026-03");

    let out = run(&[
        "scan",
        "--key",
        key.to_str().unwrap(),
        "--recursive",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "scan: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn traitor_tracing_identifies_recipient() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());

    let alice = dir.path().join("alice.rs");
    let bob = dir.path().join("bob.rs");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.rs");
    fs::copy(&src, &alice).unwrap();
    fs::copy(&src, &bob).unwrap();

    for (path, recipient) in [(&alice, "alice"), (&bob, "bob")] {
        let out = run(&[
            "embed",
            "--key",
            key.to_str().unwrap(),
            "--owner",
            "Acme",
            "--recipient",
            recipient,
            "--in-place",
            path.to_str().unwrap(),
        ]);
        assert!(out.status.success(), "{recipient}: {}", String::from_utf8_lossy(&out.stderr));
    }

    let leak = dir.path().join("suspect");
    fs::create_dir_all(&leak).unwrap();
    fs::copy(&bob, leak.join("leaked.rs")).unwrap();

    let out = run(&[
        "extract",
        "--key",
        key.to_str().unwrap(),
        "--json",
        leak.join("leaked.rs").to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["recipient"], "bob");
}

#[test]
fn strip_capacity_doctor_and_journal() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());
    let file = fixture_rs(dir.path());
    let original = fs::read_to_string(&file).unwrap();
    let journal = dir.path().join("mark.jsonl");

    let out = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme",
        "--recipient",
        "r",
        "--journal",
        journal.to_str().unwrap(),
        "--in-place",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "embed: {}", String::from_utf8_lossy(&out.stderr));
    let jtext = fs::read_to_string(&journal).unwrap();
    assert!(jtext.contains("\"owner\":\"Acme\""));
    assert!(jtext.contains("\"recipient\":\"r\""));

    let out = run(&[
        "strip",
        "--carriers",
        "comment-zw",
        "--in-place",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "strip: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    let out = run(&["capacity", file.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(!out.stdout.is_empty());

    std::fs::write(
        dir.path().join(".editorconfig"),
        "[*]\ntrim_trailing_whitespace = true\n",
    )
    .unwrap();
    let out = run(&[
        "doctor",
        "--carriers",
        "eol",
        dir.path().to_str().unwrap(),
    ]);
    assert!(out.status.success(), "doctor: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn language_examples_note_roundtrip() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());

    const FILES: &[&str] = &[
        "hello.go",
        "hello.c",
        "hello.cpp",
        "hello.java",
        "hello.js",
        "hello.ts",
        "hello.rs",
        "hello.html",
        "hello.xml",
        "hello.sgml",
        "hello.svg",
    ];
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut paths: Vec<PathBuf> = Vec::new();
    for name in FILES {
        let dest = dir.path().join(name);
        fs::copy(examples.join(name), &dest).unwrap();
        paths.push(dest);
    }

    let mut args: Vec<&str> = vec![
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme BV",
        "--note",
        "this is my message from 2026",
        "--in-place",
    ];
    let path_strs: Vec<String> = paths
        .iter()
        .map(|p| p.to_str().unwrap().to_string())
        .collect();
    for s in &path_strs {
        args.push(s.as_str());
    }
    let out = run(&args);
    assert!(
        out.status.success(),
        "embed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for path in &paths {
        let out = run(&[
            "extract",
            "--key",
            key.to_str().unwrap(),
            "--json",
            path.to_str().unwrap(),
        ]);
        assert!(
            out.status.success(),
            "extract {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["owner"], "Acme BV", "{}", path.display());
        assert_eq!(
            json["note"], "this is my message from 2026",
            "{}",
            path.display()
        );
    }
}

#[test]
fn deterministic_embed_byte_identical() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());

    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hello.go");
    let a = dir.path().join("a.go");
    let b = dir.path().join("b.go");
    let c = dir.path().join("c.go");
    let d = dir.path().join("d.go");
    fs::copy(&src, &a).unwrap();
    fs::copy(&src, &b).unwrap();
    fs::copy(&src, &c).unwrap();
    fs::copy(&src, &d).unwrap();

    let out_a = dir.path().join("out_a.go");
    let out_b = dir.path().join("out_b.go");
    for out in [&out_a, &out_b] {
        let input = if out == &out_a { &a } else { &b };
        let embed = run(&[
            "embed",
            "--key",
            key.to_str().unwrap(),
            "--owner",
            "Acme BV",
            "--note",
            "this is my message from 2026",
            "--deterministic",
            "--timestamp",
            "1735689600",
            "-o",
            out.to_str().unwrap(),
            input.to_str().unwrap(),
        ]);
        assert!(
            embed.status.success(),
            "embed: {}",
            String::from_utf8_lossy(&embed.stderr)
        );
    }
    assert_eq!(fs::read(&out_a).unwrap(), fs::read(&out_b).unwrap());

    let extract = run(&[
        "extract",
        "--key",
        key.to_str().unwrap(),
        "--json",
        out_a.to_str().unwrap(),
    ]);
    assert!(extract.status.success());
    let json: serde_json::Value = serde_json::from_slice(&extract.stdout).unwrap();
    assert_eq!(json["owner"], "Acme BV");
    assert_eq!(json["note"], "this is my message from 2026");
    assert_eq!(json["timestamp"], 1735689600);

    let out_c = dir.path().join("out_c.go");
    let out_d = dir.path().join("out_d.go");
    for (inp, out) in [(&c, &out_c), (&d, &out_d)] {
        let embed = run(&[
            "embed",
            "--key",
            key.to_str().unwrap(),
            "--owner",
            "Acme BV",
            "--note",
            "this is my message from 2026",
            "--timestamp",
            "1735689600",
            "-o",
            out.to_str().unwrap(),
            inp.to_str().unwrap(),
        ]);
        assert!(
            embed.status.success(),
            "nondet embed: {}",
            String::from_utf8_lossy(&embed.stderr)
        );
    }
    assert_ne!(fs::read(&out_c).unwrap(), fs::read(&out_d).unwrap());

    let missing_ts = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme BV",
        "--deterministic",
        "--in-place",
        a.to_str().unwrap(),
    ]);
    assert!(
        !missing_ts.status.success(),
        "deterministic without timestamp should fail"
    );
}

#[test]
fn negatives_unmarked_wrong_key_open_perms() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    let wrong = dir.path().join("wrong");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());
    assert!(run(&["keygen", "--out", wrong.to_str().unwrap()]).status.success());
    let file = fixture_rs(dir.path());

    let out = run(&[
        "extract",
        "--key",
        key.to_str().unwrap(),
        file.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));

    let out = run(&[
        "scan",
        "--key",
        key.to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));

    let out = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "A",
        "--in-place",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success());

    let out = run(&[
        "extract",
        "--key",
        wrong.to_str().unwrap(),
        file.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(&key).unwrap();
        let mut perms = meta.permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&key, perms).unwrap();
        let out = run(&[
            "extract",
            "--key",
            key.to_str().unwrap(),
            file.to_str().unwrap(),
        ]);
        assert!(!out.status.success(), "open key perms should be refused");
    }
}

#[test]
fn html_capacity_embed_extract_autodetect() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hello.html");
    let dest = dir.path().join("hello.html");
    fs::copy(&src, &dest).unwrap();

    let out = run(&[
        "capacity",
        "--carriers",
        "comment-zw",
        dest.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "capacity: {}", String::from_utf8_lossy(&out.stderr));

    let out = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme BV",
        "--note",
        "html-note",
        "--carriers",
        "comment-zw",
        "--in-place",
        dest.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "embed: {}", String::from_utf8_lossy(&out.stderr));

    let out = run(&[
        "extract",
        "--key",
        key.to_str().unwrap(),
        "--json",
        dest.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "extract: {}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["note"], "html-note");
}

#[test]
fn lang_xml_override_on_html_extension() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());
    let dest = dir.path().join("forced.html");
    fs::write(
        &dest,
        "<?xml version=\"1.0\"?>\n<root><!-- carrier alpha bravo charlie delta echo foxtrot golf hotel --></root>\n",
    )
    .unwrap();
    let out = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme",
        "--lang",
        "xml",
        "--carriers",
        "comment-zw",
        "--in-place",
        dest.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "embed: {}", String::from_utf8_lossy(&out.stderr));
    let out = run(&[
        "extract",
        "--key",
        key.to_str().unwrap(),
        "--lang",
        "xml",
        dest.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn scan_html_skips_vue() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());
    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let page = nested.join("page.html");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hello.html"),
        &page,
    )
    .unwrap();
    fs::write(dir.path().join("app.vue"), "<!-- not scanned -->\n<template></template>\n").unwrap();
    let out = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme",
        "--carriers",
        "comment-zw",
        "--in-place",
        page.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = run(&[
        "scan",
        "--key",
        key.to_str().unwrap(),
        "--recursive",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("page.html"), "{stdout}");
    assert!(!stdout.contains("app.vue"), "{stdout}");
}

#[test]
fn html_eof_comment_delimiters() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());
    let dest = dir.path().join("hello.html");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hello.html"),
        &dest,
    )
    .unwrap();
    let out = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme",
        "--carriers",
        "eof",
        "--eof-comment",
        "--in-place",
        dest.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let marked = fs::read_to_string(&dest).unwrap();
    assert!(marked.trim_end().ends_with("-->"), "{marked}");
    assert!(!marked.contains("/*"));
}

#[test]
fn doctor_warns_on_html() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.html"), "<p><!-- c --></p>\n").unwrap();
    let out = run(&[
        "doctor",
        "--carriers",
        "comment-zw",
        dir.path().to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_ascii_lowercase().contains("minifier")
            || stdout.contains("xmllint")
            || stdout.contains("tidy"),
        "{stdout}"
    );
}

#[test]
fn binary_plist_rejected() {
    let dir = tempdir().unwrap();
    let key = dir.path().join("key");
    assert!(run(&["keygen", "--out", key.to_str().unwrap()]).status.success());
    let plist = dir.path().join("x.plist");
    fs::write(&plist, b"bplist00\x00not xml").unwrap();
    let out = run(&[
        "embed",
        "--key",
        key.to_str().unwrap(),
        "--owner",
        "Acme",
        "--in-place",
        plist.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("binary plist") || err.contains("bplist"), "{err}");
}

#[test]
fn version_names_gpl_v2_only() {
    let out = run(&["--version"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("GNU General Public License") && text.contains("version 2"),
        "expected GPL v2 notice in --version: {text}"
    );
    assert!(!text.contains("MIT"), "--version must not mention MIT: {text}");
    assert!(
        !text.to_ascii_lowercase().contains("or later"),
        "--version must not say or later: {text}"
    );
}
