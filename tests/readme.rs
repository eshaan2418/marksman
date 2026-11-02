//! The examples in README.md, kept compiling and correct.
use marksman::{Document, Event, NodeValue};

#[test]
fn readme_examples() {
    let html = marksman::to_html("Hello *world* — see [the spec](https://spec.commonmark.org)\n");
    assert_eq!(
        html,
        "<p>Hello <em>world</em> — see <a href=\"https://spec.commonmark.org\">the spec</a></p>\n"
    );

    let doc = Document::parse("# A\n\ntext\n\n## B\n");
    let mut levels = vec![];
    doc.walk(|e| {
        if let Event::Enter(NodeValue::Heading { level }) = e {
            levels.push(*level)
        }
    });
    assert_eq!(levels, [1, 2]);
}
