//! Inputs that make naive Markdown parsers quadratic or overflow the stack (the categories
//! cmark's pathological test suite covers). Each must scale linearly: doubling the input may
//! at most roughly double the time. Run with `cargo test --release --test pathological`.

use std::time::{Duration, Instant};

fn time(input: &str) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let t = Instant::now();
        std::hint::black_box(marksman::to_html(input));
        best = best.min(t.elapsed());
    }
    best
}

fn assert_linear(name: &str, make: impl Fn(usize) -> String) {
    let small = make(20_000);
    let large = make(80_000);
    let (ts, tl) = (time(&small), time(&large));
    let ratio = tl.as_secs_f64() / ts.as_secs_f64().max(1e-6);
    println!("{name:<32} 20k: {ts:>10.2?}  80k: {tl:>10.2?}  ratio {ratio:.1} (linear = 4)");
    assert!(
        ratio < 10.0,
        "{name}: 4x input took {ratio:.1}x time (quadratic?)"
    );
    assert!(tl < Duration::from_secs(5), "{name}: {tl:?}");
}

#[test]
fn pathological_inputs_scale_linearly() {
    assert_linear("nested strong/emph", |n| {
        format!("{}b{}", "*a **a ".repeat(n), " a** a*".repeat(n))
    });
    assert_linear("many emph closers, no openers", |n| "a_ ".repeat(n));
    assert_linear("many emph openers, no closers", |n| "_a ".repeat(n));
    assert_linear("many link closers, no openers", |n| "a]".repeat(n));
    assert_linear("many link openers, no closers", |n| "[a".repeat(n));
    assert_linear("mismatched openers and closers", |n| "*a_ ".repeat(n));
    assert_linear("openers/closers multiple of 3", |n| {
        format!("a**b{}", "c* ".repeat(n))
    });
    assert_linear("link openers and emph closers", |n| "[ a_".repeat(n));
    assert_linear("nested brackets", |n| {
        format!("{}a{}", "[".repeat(n), "]".repeat(n))
    });
    assert_linear("nested block quotes", |n| format!("{}a", "> ".repeat(n)));
    assert_linear("unclosed inline links <", |n| "[a](<b".repeat(n / 2));
    assert_linear("unclosed inline links", |n| "[a](b".repeat(n / 2));
    // Runs of length 1..m with m ~ sqrt(2n), so the input size grows linearly with n.
    assert_linear("backticks of growing length", |n| {
        (1..((2 * n) as f64).sqrt() as usize)
            .map(|k| format!("e{}", "`".repeat(k)))
            .collect()
    });
    assert_linear("unclosed backticks", |n| "a`b".repeat(n));
    assert_linear("many references", |n| {
        (0..n / 4)
            .map(|i| format!("[{i}]: /url\n"))
            .collect::<String>()
            + "[0] [1] [2]"
    });
    // Depth m ~ sqrt(2n) so the input (sum of indentation) grows linearly with n.
    assert_linear("nested lists", |n| {
        (0..((2 * n) as f64).sqrt() as usize)
            .map(|i| format!("{}* a\n", " ".repeat(2 * i)))
            .collect()
    });
    assert_linear("unclosed HTML comments", |n| "<!-- ".repeat(n / 2));
}
