//! Any byte string that is valid UTF-8 must render without panicking. The output must also be
//! stable: rendering the same input twice gives identical HTML.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let a = marksman::to_html(s);
        assert_eq!(a, marksman::to_html(s));
    }
});
