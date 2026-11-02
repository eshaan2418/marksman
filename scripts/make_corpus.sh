#!/usr/bin/env bash
# Benchmark corpora (generated, not committed):
#  - docs.md: the README and docs/*.md of this author's published repositories (MIT), x8.
#  - spec.md: every CommonMark 0.31.2 spec example concatenated (feature-dense), x8.
set -euo pipefail
OUT=bench/corpus; mkdir -p "$OUT"
: > "$OUT/docs.md"
for r in heapwise covertrack trawl strand kleene shortlist; do
  for f in README.md docs/DESIGN.md docs/DEEP_DIVE.md; do
    curl -sSfL "https://raw.githubusercontent.com/eshaan2418/$r/main/$f" >> "$OUT/docs.one" 2>/dev/null || true
    printf '\n\n' >> "$OUT/docs.one"
  done
done
for _ in $(seq 1 8); do cat "$OUT/docs.one" >> "$OUT/docs.md"; done
rm -f "$OUT/docs.one"
scripts/fetch_spec.sh > /dev/null
python3 -c "
import json
ex = json.load(open('tests/data/spec-0.31.2.json'))
one = ''.join(e['markdown'] + '\n\n' for e in ex)
open('$OUT/spec.md', 'w').write(one * 8)
"
wc -c "$OUT"/*.md
