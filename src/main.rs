//! `marksman [FILE]`: render CommonMark from a file or stdin to HTML on stdout.
use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    let mut input = String::new();
    match std::env::args().nth(1) {
        Some(path) => input = std::fs::read_to_string(path)?,
        None => {
            io::stdin().read_to_string(&mut input)?;
        }
    }
    io::stdout().write_all(marksman::to_html(&input).as_bytes())
}
