# codestego user guide

**Version 0.9.0** (first public release).

Keyed steganographic watermarking for **source code** (C, C++, Go, Java, JavaScript, TypeScript, Rust, HTML, XML, SGML, and XML vocabs such as SVG).

Embed a short, encrypted payload (owner, recipient, note, timestamp) into comments and insignificant whitespace. Extraction requires your secret key. Per-recipient payloads support **traitor tracing**: if a partner leaks your code, the mark says who received that copy.

## Threat model

**Defends against:** someone who receives or steals your source, reuses it, and claims independent authorship. You extract the mark from their copy; it decrypts and authenticates only under your key.

**Does not defend against:** an adversary who suspects watermarking and runs a sanitizer (strip trailing whitespace, strip non-ASCII from comments, delete all comments). Robustness targets **accidental** destruction (formatters, editors, partial copies), not a motivated eraser.

Presence of a mark is proven only by successful keyed authentication — there are no plaintext magic bytes.

## Install

Requires Rust 1.85+ (edition 2024).

### From source

```bash
cargo install --path .
# or
cargo run -- <command>
```

### From packages

Build packages on a development machine (see [Packaging from source](#packaging-from-source)), then:

```bash
# Debian / Ubuntu
sudo dpkg -i target/debian/codestego_*.deb

# Fedora / RHEL / openSUSE
sudo rpm -i target/generate-rpm/codestego-*.rpm
```

After install, the binary is `/usr/bin/codestego`. This guide is installed at `/usr/share/doc/codestego/USER_GUIDE.md` and the man page as `man codestego`.

## Key management

```bash
# Raw random key (file mode 0600). Keep it secret.
codestego keygen --out ~/.config/codestego/key

# Passphrase-protected key (interactive stdin; derived key is never stored)
codestego keygen --out ~/.config/codestego/key --passphrase
```

- Key files with group/other permissions are **refused**.
- CI can set `CODESTEGO_KEY` to a hex-encoded 32-byte key instead of a file (used when `--key` is omitted).
- Commands that need a key accept `--key PATH` and optional `--passphrase STRING` for passphrase files.

## Everyday workflow

```bash
# 1. Capacity check (comment-zw needs comments)
codestego capacity src/main.rs

# 2. Embed a per-recipient mark
codestego embed --key ~/.config/codestego/key \
  --owner "Acme BV" --recipient "partner-x" --note "license-2026-03" \
  --in-place src/main.rs

# 3. Extract
codestego extract --key ~/.config/codestego/key src/main.rs
# Machine-readable:
codestego extract --key ~/.config/codestego/key --json src/main.rs

# 4. Scan a suspect tree (exit 0 = mark found, 1 = not found)
codestego scan --key ~/.config/codestego/key --recursive /path/to/suspect
```

For a per-language how-to that embeds a note into Go/C/C++/Java/JS/TS/Rust/HTML/XML/SGML/SVG, see [examples/](../examples/).

Every **embed** re-lexes and asserts the non-trivia token stream is unchanged, then round-trip extracts before writing (temp + rename). Optional `--compile-check 'cc -fsyntax-only {}'`. Optional `--journal path.jsonl` appends BLAKE3 hashes + HMAC evidence records.

## Carriers and modes

Select with `--carriers a,b,c`. Default embed carrier: `comment-zw`.

| Carrier | How it encodes | Survives | Dies to |
|---------|----------------|----------|---------|
| `comment-zw` | Zero-width chars in comments | gofmt, prettier, clang-format (in practice) | Non-ASCII scrubbers, deleting comments |
| `comment-space` | 1 vs 2 spaces between words in comments | Mild reformats that preserve spaces | Comment re-wrapping |
| `eol` | Trailing space / tab on eligible lines | Almost nothing | trim-on-save, mainstream formatters |
| `eof` | Trailing blank lines or `--eof-comment` block | Rarely | Most cleanups; overflow / tiny files |

- `--carrier-mode replicate` (default): full independent copy in each selected carrier. Capacity is the **min** of selected carriers. A formatter that kills `eol` can leave `comment-zw` intact.
- `--carrier-mode chain`: concatenate capacity across carriers for large payloads in small files.
- `--ascii-only` disables `comment-zw` (and ZW eof-comments).
- `--zw-alphabet zw4|vs16` and `--zw-per-comment N` (default 1024) control zero-width density.

### Strip semantics

- `comment-zw` / `eof`: strip aims for **byte-identical** restore of the original unmarked file.
- `eol`: trailing whitespace is **removed** (normalized).
- `comment-space`: inter-word runs are **normalized** to a single space (not always identical to the pre-embed original if the original already had double spaces).

## Traitor tracing

1. Generate one master key; never share it.
2. For each partner or build, embed with a unique `--recipient` (and optional `--note` / `--journal`).
3. If code appears elsewhere, `scan --recursive` their tree.
4. Extracted `recipient` identifies the leak channel. Journal entries prove when you marked that copy.

## Commands reference

```
codestego keygen [--out PATH] [--passphrase]
codestego embed  --owner NAME [--recipient R] [--note N] [carrier options...]
                 [--deterministic --timestamp SECONDS]
                 [--in-place | -o OUT] [--dry-run] [--journal PATH]
                 [--compile-check 'CMD {}'] PATHS...
codestego extract [--carriers auto] [--deep] [--json] PATHS...
codestego scan    [--recursive] [--json] [--deep] PATHS...
codestego capacity [--carriers ...] PATHS...
codestego strip   [--carriers all] [--in-place | -o OUT] PATHS...
codestego doctor  [--carriers ...] [PATHS...]
```

Common embed/extract options: `--key`, `--passphrase`, `--carriers`, `--carrier-mode`, `--parity`, `--shard-size`, `--ascii-only`, `--zw-alphabet`, `--zw-per-comment`, `--eol-bits`, `--eof-comment`, `--lang`.

Default embeds use a random AEAD nonce and wall-clock timestamp, so marked source bytes differ across runs. For byte-identical **source** output (demos/golden files), use `--deterministic` with a required `--timestamp SECONDS`. `--timestamp` alone pins the payload time while keeping a random nonce. Journal `unix_logged` is always wall-clock and is not made deterministic.

Languages are detected by extension (`*.c`, `*.cpp`, `*.go`, `*.java`, `*.js`/`*.ts`, `*.rs`, `*.html`, `*.xml`/`*.svg` and other XML vocabs, `*.sgml`) with `--lang` override (`html`, `xml`, `sgml`, …). HTML comments are `<!-- -->`; `<script>`/`<style>`/`<iframe>`/`<textarea>`/`<title>` are not a comment channel. XML `/>` is an empty element. SGML uses XML-style `<!-- -->` comments. Binary `.plist` files are rejected.

## Doctor and formatters

`codestego doctor` looks for `.editorconfig` `trim_trailing_whitespace`, Prettier/clang-format/rustfmt configs, gofmt exposure, and pre-commit hooks that would destroy chosen carriers. Treat warnings as guidance before batch marking a repo.

## Safety and limits

- Embed aborts if verification fails; no partial write of a broken file.
- Key files with group/other permissions are refused.
- Passphrase keys use Argon2id; the derived key is never stored.
- Capacity guidance: a typical short payload with default shard size and parity is on the order of ~2 KiB of bits. Use `capacity` before batch marking. For tiny files without comments, add `--carriers eof --eof-comment` or insert a harmless comment first.

## Packaging from source

Host tools (one-time):

```bash
cargo install cargo-deb
cargo install cargo-generate-rpm
```

Then:

```bash
make deb      # → target/debian/*.deb
make rpm      # → target/generate-rpm/*.rpm (builds release + optional strip first)
make packages # both
```

`cargo-generate-rpm` does **not** require `rpmbuild`. Requires a prior release binary (`make rpm` runs `cargo build --release`).

## Development

```bash
make test
# or
cargo test

# Optional coverage HTML (host tool):
cargo install cargo-llvm-cov
cargo llvm-cov --html
```

## Copyright

Copyright (C) 2026 Maurice Gittens. Licensed under the GNU General Public License, version 2. See LICENSE.
