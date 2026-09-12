#!/usr/bin/env bash
# Sandbox: embed/extract the demo note on copies of hello.* (does not touch
# examples/ or ~/.config/codestego/key).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXAMPLES="$ROOT/examples"
NOTE="this is my message from 2026"
OWNER="Acme BV"
FILES=(hello.go hello.c hello.cpp hello.java hello.js hello.ts hello.rs hello.html hello.xml hello.sgml hello.svg)

if command -v codestego >/dev/null 2>&1; then
  CODESTEGO=(codestego)
else
  CODESTEGO=(cargo run --quiet --manifest-path "$ROOT/Cargo.toml" --)
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

KEY="$WORKDIR/key"
"${CODESTEGO[@]}" keygen --out "$KEY"

for f in "${FILES[@]}"; do
  cp "$EXAMPLES/$f" "$WORKDIR/$f"
done

paths=()
for f in "${FILES[@]}"; do
  paths+=("$WORKDIR/$f")
done

"${CODESTEGO[@]}" embed --key "$KEY" \
  --owner "$OWNER" \
  --note "$NOTE" \
  --in-place \
  "${paths[@]}"

for f in "${FILES[@]}"; do
  echo "=== $f ==="
  "${CODESTEGO[@]}" extract --key "$KEY" "$WORKDIR/$f"
done
