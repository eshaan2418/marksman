//! Spec conformance of marksman, pulldown-cmark, and comrak on the CommonMark 0.31.2 examples.
type Engine = (&'static str, fn(&str) -> String);

fn main() {
    let raw =
        std::fs::read_to_string("tests/data/spec-0.31.2.json").expect("run scripts/fetch_spec.sh");
    let ex: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
    let engines: [Engine; 3] = [
        ("marksman", marksman::to_html),
        ("pulldown-cmark 0.13.3", |t| {
            let mut out = String::new();
            pulldown_cmark::html::push_html(
                &mut out,
                pulldown_cmark::Parser::new_ext(t, pulldown_cmark::Options::empty()),
            );
            out
        }),
        ("comrak 0.47.0", |t| {
            comrak::markdown_to_html(t, &comrak::Options::default())
        }),
    ];
    for (name, f) in engines {
        let pass = ex
            .iter()
            .filter(|e| f(e["markdown"].as_str().unwrap()) == e["html"].as_str().unwrap())
            .count();
        println!("{name}: {pass}/{} examples identical to the spec", ex.len());
    }
}
