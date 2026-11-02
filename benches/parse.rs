//! Parse + render throughput: marksman vs pulldown-cmark vs comrak (CommonMark mode, no extensions).
//! Corpora come from scripts/make_corpus.sh.
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn load(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/bench/corpus/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("run scripts/make_corpus.sh first"))
}

fn bench(c: &mut Criterion) {
    for name in ["docs.md", "spec.md"] {
        let text = load(name);
        let mut g = c.benchmark_group(name);
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.sample_size(30);
        g.bench_with_input(BenchmarkId::new("marksman", name), &text, |b, t| {
            b.iter(|| marksman::to_html(t))
        });
        g.bench_with_input(BenchmarkId::new("pulldown-cmark", name), &text, |b, t| {
            b.iter(|| {
                let mut out = String::with_capacity(t.len() * 3 / 2);
                pulldown_cmark::html::push_html(
                    &mut out,
                    pulldown_cmark::Parser::new_ext(t, pulldown_cmark::Options::empty()),
                );
                out
            })
        });
        g.bench_with_input(BenchmarkId::new("comrak", name), &text, |b, t| {
            b.iter(|| comrak::markdown_to_html(t, &comrak::Options::default()))
        });
        g.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
