"""Regenerate src/entities.rs from the WHATWG entities.json (CC BY 4.0, (c) WHATWG).

Usage: curl -sSfL -o entities.json https://html.spec.whatwg.org/entities.json && python3 scripts/gen_entities.py entities.json
"""

import json
import sys


def lit(s: str) -> str:
    return '"' + "".join(f"\\u{{{ord(c):x}}}" if (ord(c) < 0x20 or ord(c) > 0x7E or c in '"\\') else c for c in s) + '"'


def main(path: str) -> None:
    d = json.load(open(path))
    items = sorted((k[1:-1], v["characters"]) for k, v in d.items() if k.endswith(";"))
    with open("src/entities.rs", "w") as f:
        f.write("// Generated from the WHATWG HTML Standard named character references\n")
        f.write("// (https://html.spec.whatwg.org/entities.json, (c) WHATWG, CC BY 4.0) by scripts/gen_entities.py.\n")
        f.write("// Only names with a trailing semicolon are included: CommonMark requires the semicolon.\n\n")
        f.write("/// Sorted by name for binary search.\npub(crate) static ENTITIES: &[(&str, &str)] = &[\n")
        for n, s in items:
            f.write(f'    ("{n}", {lit(s)}),\n')
        f.write("];\n\n/// Look up a named reference (without `&` and `;`).\n")
        f.write("pub(crate) fn lookup(name: &str) -> Option<&'static str> {\n")
        f.write("    ENTITIES.binary_search_by(|(n, _)| (*n).cmp(name)).ok().map(|i| ENTITIES[i].1)\n}\n")


if __name__ == "__main__":
    main(sys.argv[1])
