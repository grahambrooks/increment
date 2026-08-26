//! `--format json`.
//!
//! The one serializer. Every surface — the CLI, a machine reader, and later the
//! TUI — describes a diff through this function rather than each inventing its
//! own shape, which is the only way three surfaces stay honest about what a
//! diff contains.

use std::io::Write;

use crate::model::DiffDocument;

pub fn render(document: &DiffDocument, out: &mut impl Write) -> std::io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, document)?;
    writeln!(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{Options, compare};
    use crate::model::SourceFile;

    #[test]
    fn the_document_round_trips_through_json() {
        let old = SourceFile::from_text("old", "keep\nlet x = 1;\ndrop\n");
        let new = SourceFile::from_text("new", "keep\nlet x = 2;\n");
        let document = compare(&old, &new, &Options::default());

        let mut buffer = Vec::new();
        render(&document, &mut buffer).expect("renders");

        let parsed: DiffDocument = serde_json::from_slice(&buffer).expect("parses back");
        assert_eq!(parsed, document);
    }

    #[test]
    fn a_row_names_its_kind_and_carries_its_emphasis() {
        let old = SourceFile::from_text("old", "let x = 1;\n");
        let new = SourceFile::from_text("new", "let x = 2;\n");
        let document = compare(&old, &new, &Options::default());

        let mut buffer = Vec::new();
        render(&document, &mut buffer).expect("renders");
        let text = String::from_utf8(buffer).expect("utf-8");

        assert!(text.contains(r#""kind": "modified""#), "{text}");
        assert!(text.contains(r#""emphasis""#), "{text}");
    }
}
