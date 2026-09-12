# Language examples

These unmarked sources show how to hide a short note inside Go, C, C++, Java,
JavaScript, TypeScript, Rust, HTML, XML, SGML, and SVG with the default
`comment-zw` carrier.

The note is **encrypted into comments** (zero-width characters). It will **not**
appear as plaintext in the file. Put your message in `--note`; `--owner` is a
required label (same as the project docs).

Do **not** run `--in-place` on the tracked `hello.*` files here — that dirties
git. Prefer `-o` for a single file, or use the sandbox script below.

## Copy-paste workflow (one file)

```bash
test -f ~/.config/codestego/key || codestego keygen --out ~/.config/codestego/key

codestego capacity --carriers comment-zw examples/hello.html

codestego embed --key ~/.config/codestego/key \
  --owner "Acme BV" \
  --note "this is my message from 2026" \
  -o /tmp/hello.marked.html examples/hello.html

codestego extract --key ~/.config/codestego/key /tmp/hello.marked.html
```

Expected extract output includes `note="this is my message from 2026"` (and an
empty `recipient` when you omit `--recipient`).

Default embeds are **not** byte-identical across runs (random nonce + wall-clock
time). For reproducible marked **source** files:

```bash
codestego embed --key ~/.config/codestego/key \
  --owner "Acme BV" \
  --note "this is my message from 2026" \
  --deterministic --timestamp 1735689600 \
  -o /tmp/hello.marked.html examples/hello.html
```

`--deterministic` requires `--timestamp`. Journal lines (if used) are still
non-deterministic.

Use `capacity --carriers comment-zw` (not bare `capacity`): the default capacity
command mins all carriers, which under-reports tiny files even when default
embed succeeds.

## Sandbox (all languages)

`./embed.sh` copies the eleven `hello.*` sources into a temp directory, generates
a **throwaway** key, embeds the same note in-place there, and extracts each
file. It never writes back into `examples/` or `~/.config/codestego/key`.

```bash
./examples/embed.sh
```

## Languages

| Language   | File            | Extensions (auto-detect) | `--lang` |
|------------|-----------------|--------------------------|----------|
| Go         | `hello.go`      | `.go`                    | `go`     |
| C          | `hello.c`       | `.c`, `.h`               | `c`      |
| C++        | `hello.cpp`     | `.cpp`, `.cc`, `.cxx`, … | `cpp`    |
| Java       | `hello.java`    | `.java`                  | `java`   |
| JavaScript | `hello.js`      | `.js`, `.jsx`, `.mjs`, … | `js`     |
| TypeScript | `hello.ts`      | `.ts`, `.tsx`, `.mts`, … | `ts`     |
| Rust       | `hello.rs`      | `.rs`                    | `rust`   |
| HTML       | `hello.html`    | `.html`, `.htm`          | `html`   |
| XML        | `hello.xml`     | `.xml` and XML vocabs    | `xml`    |
| SGML       | `hello.sgml`    | `.sgml`, `.sgm`          | `sgml`   |
| SVG        | `hello.svg`     | `.svg` (XML profile)     | `xml`    |

Override with `--lang` when the extension is wrong. Each file has two comments
so `comment-zw` has capacity margin for the encrypted frame.

HTML comments are `<!-- -->`. Content of `<script>`, `<style>`, `<iframe>`,
`<textarea>`, and `<title>` is not a comment channel (including after `/>`).
XML/SVG `/>` is an empty element. SGML uses the same `<!-- -->` comments as XML
(not DTD `-- --` declaration comments). HTML minifiers that strip comments
destroy `comment-zw`. Binary `.plist` files are rejected.
