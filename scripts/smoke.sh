#!/usr/bin/env bash
# Minimal end-to-end check of a built binary: used by CI and the release workflow.
# Usage: scripts/smoke.sh path/to/pdftk-rs
set -euo pipefail
bin=$1
fx=tests/files
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

"$bin" --version | head -1
"$bin" "$fx/refs.pdf" cat 1 end output "$tmp/cat.pdf"
"$bin" A="$tmp/cat.pdf" B="$fx/blank.pdf" shuffle A B output "$tmp/shuffle.pdf" owner_pw o user_pw u 2>/dev/null
"$bin" "$tmp/shuffle.pdf" input_pw u dump_data output "$tmp/data.txt" 2>/dev/null
grep -q '^NumberOfPages: 3$' "$tmp/data.txt"
"$bin" "$fx/form.pdf" fill_form "$fx/form-filled.fdf" output "$tmp/filled.pdf" flatten
"$bin" "$fx/form.pdf" dump_data_fields | grep -q '^FieldName: '
"$bin" "$fx/refs.pdf" burst output "$tmp/pg_%02d.pdf"
test -f "$tmp/pg_01.pdf"
"$bin" "$fx/refs.pdf" stamp "$fx/duck.pdf" output "$tmp/stamped.pdf"
"$bin" "$fx/refs.pdf" attach_files "$fx/form.fdf" output "$tmp/att.pdf"
mkdir "$tmp/unpacked"
"$bin" "$tmp/att.pdf" unpack_files output "$tmp/unpacked"
cmp "$fx/form.fdf" "$tmp/unpacked/form.fdf"
echo "smoke test ok"
