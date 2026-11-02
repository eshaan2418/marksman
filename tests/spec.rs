//! Runs every example from the CommonMark 0.31.2 spec (fetch with scripts/fetch_spec.sh).
//! Output must match the expected HTML byte for byte.

use std::collections::BTreeMap;

#[test]
fn commonmark_spec() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/spec-0.31.2.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        panic!("missing {path}: run scripts/fetch_spec.sh");
    };
    let examples: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("valid spec JSON");
    let mut by_section: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    for ex in &examples {
        let md = ex["markdown"].as_str().unwrap();
        let want = ex["html"].as_str().unwrap();
        let section = ex["section"].as_str().unwrap().to_string();
        let got = marksman::to_html(md);
        let e = by_section.entry(section.clone()).or_default();
        e.1 += 1;
        if got == want {
            e.0 += 1;
        } else {
            failures.push((
                ex["example"].as_u64().unwrap(),
                section,
                md.to_string(),
                want.to_string(),
                got,
            ));
        }
    }
    let passed: usize = by_section.values().map(|v| v.0).sum();
    println!("passed {passed}/{}", examples.len());
    for (s, (p, t)) in &by_section {
        if p != t {
            println!("  {s}: {p}/{t}");
        }
    }
    for (n, s, md, want, got) in failures.iter().take(usize::MAX) {
        println!("--- example {n} ({s})\n{md:?}\nwant {want:?}\ngot  {got:?}");
    }
    assert!(
        failures.is_empty(),
        "{} of {} spec examples failed",
        failures.len(),
        examples.len()
    );
}
