#!/usr/bin/env bash
# Fetch the CommonMark 0.31.2 spec examples (CC BY-SA 4.0, not redistributed here).
set -euo pipefail
mkdir -p tests/data
f=tests/data/spec-0.31.2.json
[[ -f $f ]] || curl -sSfL -o "$f" https://spec.commonmark.org/0.31.2/spec.json
echo "d431b29d97b6f73e69d547109cf5081578fac931e72afe95639ebe766c1b2a20  $f" | shasum -a 256 -c -
