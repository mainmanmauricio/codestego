# codestego

Keyed steganographic watermarking for **source code** (C, C++, Go, Java, JavaScript, TypeScript, Rust, HTML, XML, SGML, and XML vocabs such as SVG).

Embed a short, encrypted payload (owner, recipient, note, timestamp) into comments and insignificant whitespace. Extraction requires your secret key. Per-recipient payloads support **traitor tracing**: if a partner leaks your code, the mark says who received that copy.

**Version:** 0.9.0 (first public release) · **Full documentation:** [docs/USER_GUIDE.md](docs/USER_GUIDE.md) · `man codestego` (when installed from packages).

## Threat model (honest)

**Defends against:** someone who receives or steals your source, reuses it, and claims independent authorship. You extract the mark from their copy; it decrypts and authenticates only under your key.

**Does not defend against:** an adversary who suspects watermarking and runs a sanitizer (strip trailing whitespace, strip non-ASCII from comments, delete all comments). No comment/whitespace channel survives a determined stripper. Robustness targets **accidental** destruction (formatters, editors, partial copies), not a motivated eraser.

Presence of a mark is proven only by successful keyed authentication — there are no plaintext magic bytes.

## Install

Requires Rust 1.85+ (edition 2024). With rustup:

```bash
cd ~/projects/codestego
cargo install --path .
```

Or run from the tree:

```bash
cargo run -- <command>
```

## Quick start

```bash
# 1. Generate a key (mode 0600). Keep it secret.
codestego keygen --out ~/.config/codestego/key

# 2. Check capacity (needs comments for comment-zw)
codestego capacity src/main.rs

# 3. Embed a per-recipient mark
codestego embed --key ~/.config/codestego/key \
  --owner "Acme BV" --recipient "partner-x" --note "license-2026-03" \
  --in-place src/main.rs

# 4. Extract
codestego extract --key ~/.config/codestego/key src/main.rs

# 5. Scan a suspect tree (exit 0 = mark found)
codestego scan --key ~/.config/codestego/key --recursive /path/to/suspect
```

For a per-language how-to that embeds a note into Go/C/C++/Java/JS/TS/Rust/HTML/XML/SGML/SVG, see [examples/](examples/).

CI can use `CODESTEGO_KEY` (hex-encoded 32-byte key) instead of a file.

## Carriers

Select with `--carriers a,b,c`. Default embed carrier: `comment-zw`.

| Carrier | How it encodes | Survives | Dies to |
|---------|----------------|----------|---------|
| `comment-zw` | Zero-width chars (U+200B/C/D, U+2060) appended inside comments; decoded in document order | gofmt, prettier, clang-format (including comment reflow) in practice | Non-ASCII scrubbers, deleting comments |
| `comment-space` | 1 vs 2 spaces between words in comments | Mild reformats that preserve spaces | Comment re-wrapping |
| `eol` | Trailing space / tab on eligible lines | Almost nothing | trim-on-save, every mainstream formatter |
| `eof` | Trailing blank lines (space-count = byte) or `--eof-comment` block | Rarely | Most cleanups; intended as overflow |

`--carrier-mode replicate` (default): full independent copy in each selected carrier (a formatter that kills `eol` leaves `comment-zw` intact). Capacity is the **min** of selected carriers.

`--carrier-mode chain`: concatenate capacity across carriers for large payloads in small files.

`--ascii-only` disables `comment-zw` (and ZW eof-comments).

`--zw-alphabet zw4|vs16` and `--zw-per-comment N` (default 1024) control zero-width density.

## Wire format (summary)

1. Payload TLV: version, unix timestamp, owner / recipient / note (length-prefixed UTF-8).
2. Capsule: `nonce(24) || XChaCha20Poly1305(payload)` with AAD `codestego/v1`.
3. Length-prefix, pad, split into `k` shards of `--shard-size` (default 24); add `m = ceil(k * --parity)` Reed-Solomon parity shards (default parity `1.0` → recover from any half).
4. Each block: obfuscated 8-byte keyed header + shard body. No plaintext magic.

## Commands

```
codestego keygen [--out PATH] [--passphrase]
codestego embed  --owner NAME [--recipient R] [--note N] [carrier options...]
                 [--deterministic --timestamp SECONDS] [--in-place | -o OUT] PATHS...
codestego extract [--carriers auto] [--deep] [--json] PATHS...
codestego scan    [--recursive] [--json] PATHS...     # exit 0 if found
codestego capacity [--carriers ...] PATHS...
codestego strip   [--carriers all] [--in-place | -o OUT] PATHS...
codestego doctor  [--carriers ...] [PATHS...]         # formatter conflict warnings
```

Every **embed** re-lexes and asserts the non-trivia token stream is unchanged, then round-trip extracts before writing. Optional `--compile-check 'cc -fsyntax-only {}'`. Optional `--journal path.jsonl` appends BLAKE3 hashes + HMAC evidence records.

Default embeds use a random AEAD nonce and wall-clock timestamp (non-identical bytes across runs). For reproducible **source** files (demos/golden tests), pass `--deterministic --timestamp SECONDS` (timestamp is required). Journal lines remain non-deterministic even then.

`doctor` looks for `.editorconfig` `trim_trailing_whitespace`, Prettier/clang-format/rustfmt configs, gofmt exposure, and pre-commit hooks that would destroy chosen carriers.

## Traitor-tracing workflow

1. Generate one master key; never share it.
2. For each partner or build, embed with a unique `--recipient` (and optional `--note` / journal).
3. If code appears elsewhere, `scan --recursive` their tree.
4. Extracted `recipient` identifies the leak channel. Journal entries prove when you marked that copy.

## Capacity guidance

A typical short payload (“Acme BV” / “partner-x”) with default shard size and parity ≈ **2 KiB of bits** (~1–2 KiB of zero-width characters in comments). One normal comment with `--zw-per-comment 1024` is enough. Use `codestego capacity` before batch marking. For tiny files without comments, add `--carriers eof --eof-comment` or insert a harmless comment first.

## Languages

Detection by extension (`*.c`, `*.cpp`, `*.go`, `*.java`, `*.js`/`*.ts`, `*.rs`, `*.html`, `*.xml`/`*.svg` and other XML vocabs, `*.sgml`) with `--lang` override (`html`, `xml`, `sgml`, …). The trivia lexer understands nested Rust block comments, C++/Rust raw strings, Go backticks, JS/TS template literals, HTML/XML `<!-- -->` comments (HTML also `--!>`), and conservatively **skips** ambiguous JS regex regions and HTML `<script>`/`<style>`/`<iframe>`/`<textarea>`/`<title>` content rather than risk corrupting code. Binary `.plist` files are rejected. HTML minifiers that strip comments destroy `comment-zw`.

## Safety and limits

- Embed aborts if verification fails; no partial write of a broken file (temp + rename).
- Key files with mode group/other readable are refused.
- Passphrase keys use Argon2id; the derived key is never stored.
- `strip` restores byte-identical files for `comment-zw` / `eof`; `eol` is normalized (trailing whitespace removed).

## Packaging

```bash
cargo install cargo-deb            # once
cargo install cargo-generate-rpm   # once
make deb        # → target/debian/*.deb
make rpm        # → target/generate-rpm/*.rpm
make packages   # both
```

## Development

```bash
make test
# or: cargo test
cargo run -- --help

# Optional coverage (host tool):
# cargo install cargo-llvm-cov && cargo llvm-cov --html
```

See the [user guide](docs/USER_GUIDE.md) for workflows, carriers, packaging details, and command reference.

## License

Copyright (C) 2026 Maurice Gittens.

This program is free software; you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation; version 2 of the License only. See [LICENSE](LICENSE).

This program is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE.
