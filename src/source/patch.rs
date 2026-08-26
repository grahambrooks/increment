//! A unified diff on stdin, for `core.pager` compatibility.
//!
//! **This source is deliberately lower fidelity, and says so.** A pager is
//! handed the diff git already decided to print: a few lines of context around
//! each change and nothing else. The files themselves are not available, so
//! there is nothing to fold that git has not already folded, no whole-file
//! view, and no way to re-diff a region with different settings. Where the
//! choice exists, [`super::git`] is the better path — it reads both sides in
//! full.
//!
//! What this *does* do is re-diff each hunk's two sides, so the pairing and the
//! word-level emphasis are gdiff's rather than git's. Between hunks it emits a
//! fold carrying the gap the hunk headers imply, which is exactly as much as
//! the patch knows.

use std::io::BufRead;

use crate::diff::{self, Options as DiffOptions};
use crate::model::{DiffDocument, FileMeta, Row, RowKind, SourceFile};

use super::Changes;

/// Parse a unified diff and turn each file in it into a document.
pub fn parse(input: impl BufRead, options: &DiffOptions) -> std::io::Result<Changes> {
    let mut files: Vec<PatchFile> = Vec::new();

    for line in input.lines() {
        let line = line?;

        if let Some(name) = line.strip_prefix("--- ") {
            files.push(PatchFile::new(name.trim_end()));
            continue;
        }
        if let Some(name) = line.strip_prefix("+++ ")
            && let Some(file) = files.last_mut()
        {
            file.new_name = name.trim_end().to_owned();
            continue;
        }
        if let Some(header) = Header::parse(&line) {
            if let Some(file) = files.last_mut() {
                file.hunks.push(Hunk::new(header));
            }
            continue;
        }

        let Some(hunk) = files.last_mut().and_then(|file| file.hunks.last_mut()) else {
            // Anything before the first hunk — `diff --git`, index lines, mode
            // changes — is not content and is not ours to render.
            continue;
        };
        hunk.push(&line);
    }

    Ok(Changes {
        documents: files
            .into_iter()
            .filter(|file| !file.hunks.is_empty())
            .map(|file| file.into_document(options))
            .collect(),
        ..Changes::default()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    old_start: usize,
    new_start: usize,
}

impl Header {
    /// Parse `@@ -12,7 +12,9 @@ optional trailing context`.
    fn parse(line: &str) -> Option<Self> {
        let body = line.strip_prefix("@@ ")?;
        let body = body.split(" @@").next()?;
        let mut fields = body.split(' ');

        let start = |field: Option<&str>, sign: char| -> Option<usize> {
            let field = field?.strip_prefix(sign)?;
            let count = field.split_once(',').map_or(field, |(start, _)| start);
            count.parse().ok()
        };

        Some(Self {
            old_start: start(fields.next(), '-')?,
            new_start: start(fields.next(), '+')?,
        })
    }
}

#[derive(Debug)]
struct Hunk {
    header: Header,
    old: Vec<String>,
    new: Vec<String>,
}

impl Hunk {
    fn new(header: Header) -> Self {
        Self {
            header,
            old: Vec::new(),
            new: Vec::new(),
        }
    }

    fn push(&mut self, line: &str) {
        // A completely empty line in a patch is a context line whose content is
        // empty and whose leading space some tool trimmed. Treating it as
        // unknown would drop a real line.
        let (marker, text) = line.split_at(line.chars().next().map_or(0, char::len_utf8));
        match marker {
            " " | "" => {
                self.old.push(text.to_owned());
                self.new.push(text.to_owned());
            }
            "-" => self.old.push(text.to_owned()),
            "+" => self.new.push(text.to_owned()),
            // `\ No newline at end of file`, and anything else that is not
            // content.
            _ => {}
        }
    }
}

#[derive(Debug)]
struct PatchFile {
    old_name: String,
    new_name: String,
    hunks: Vec<Hunk>,
}

impl PatchFile {
    fn new(old_name: &str) -> Self {
        Self {
            old_name: old_name.to_owned(),
            new_name: old_name.to_owned(),
            hunks: Vec::new(),
        }
    }

    /// Re-diff each hunk and stitch the results into one document.
    fn into_document(self, options: &DiffOptions) -> DiffDocument {
        let mut rows: Vec<Row> = Vec::new();
        let (mut old_seen, mut new_seen) = (0usize, 0usize);

        for hunk in &self.hunks {
            // The lines between the previous hunk and this one exist in the
            // file but not in the patch. Their count is all that is known.
            let hidden = hunk.header.old_start.saturating_sub(old_seen + 1);
            if hidden > 0 {
                rows.push(Row::fold(hidden));
            }

            let old = SourceFile::from_text(&self.old_name, &joined(&hunk.old));
            let new = SourceFile::from_text(&self.new_name, &joined(&hunk.new));
            // No folding within a hunk: git already chose this context, and
            // folding it again would hide lines it deliberately included.
            let document = diff::compare(
                &old,
                &new,
                &DiffOptions {
                    context: None,
                    ..*options
                },
            );

            rows.extend(
                document
                    .rows
                    .into_iter()
                    .map(|row| shift(row, hunk.header.old_start - 1, hunk.header.new_start - 1)),
            );

            old_seen = hunk.header.old_start + hunk.old.len() - 1;
            new_seen = hunk.header.new_start + hunk.new.len() - 1;
        }

        DiffDocument::from_meta(
            FileMeta::new(self.old_name, old_seen),
            FileMeta::new(self.new_name, new_seen),
            rows,
        )
    }
}

fn joined(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| format!("{line}\n"))
        .collect::<String>()
}

/// Move a row's line numbers from hunk-relative to file-absolute.
fn shift(mut row: Row, old_offset: usize, new_offset: usize) -> Row {
    if let Some(line) = row.left.as_mut() {
        line.number += old_offset;
    }
    if let Some(line) = row.right.as_mut() {
        line.number += new_offset;
    }
    debug_assert!(
        !matches!(row.kind, RowKind::Fold { .. }),
        "hunks do not fold"
    );
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RowKind;

    fn documents(patch: &str) -> Vec<DiffDocument> {
        parse(patch.as_bytes(), &DiffOptions::default())
            .expect("parses")
            .documents
    }

    const SIMPLE: &str = "\
--- a/catalog.rs
+++ b/catalog.rs
@@ -6,5 +6,6 @@ fn render() {
     let page = Page::new();
     match data.get() {
-        let x = 1;
+        let x = 2;
+        let y = 3;
     }
 }
";

    #[test]
    fn a_hunk_header_gives_the_real_line_numbers() {
        let documents = documents(SIMPLE);
        assert_eq!(documents.len(), 1);

        // The patch says this hunk starts at line 6, not line 1.
        let first = documents[0]
            .rows
            .iter()
            .find(|row| row.left.is_some())
            .expect("a row with content");
        assert_eq!(first.left.as_ref().unwrap().number, 6);
        assert_eq!(first.right.as_ref().unwrap().number, 6);
    }

    #[test]
    fn both_file_names_are_taken_from_the_headers() {
        let documents = documents(SIMPLE);
        assert_eq!(documents[0].old.name, "a/catalog.rs");
        assert_eq!(documents[0].new.name, "b/catalog.rs");
    }

    #[test]
    fn the_hunk_is_re_diffed_so_emphasis_is_ours_not_gits() {
        let documents = documents(SIMPLE);
        let modified = documents[0]
            .rows
            .iter()
            .find(|row| matches!(row.kind, RowKind::Modified))
            .expect("the edited line should pair");

        let right = modified.right.as_ref().unwrap();
        assert!(
            !right.emphasis.is_empty(),
            "word-level emphasis should have been computed"
        );
    }

    #[test]
    fn the_gap_before_a_hunk_becomes_a_fold_of_the_right_size() {
        // The first hunk starts at line 6, so lines 1 to 5 exist in the file
        // but not in the patch. Saying "5 unchanged lines" is the most this
        // source can honestly claim about them.
        let document = &documents(SIMPLE)[0];
        assert!(
            matches!(document.rows[0].kind, RowKind::Fold { hidden: 5 }),
            "expected a leading fold of 5, got {:?}",
            document.rows[0].kind
        );
        assert_eq!(folds(document), [5]);
    }

    fn folds(document: &DiffDocument) -> Vec<usize> {
        document
            .rows
            .iter()
            .filter_map(|row| match row.kind {
                RowKind::Fold { hidden } => Some(hidden),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn two_hunks_are_separated_by_a_fold_of_the_lines_between_them() {
        let patch = "\
--- a/x
+++ b/x
@@ -1,2 +1,2 @@
 one
-two
+TWO
@@ -20,2 +20,2 @@
 twenty
-twentyone
+TWENTYONE
";
        // The first hunk covers old lines 1 and 2; the second starts at 20. So
        // lines 3 to 19 are hidden — seventeen of them. Counted from the body
        // rather than from the header's claimed length, because the two can
        // disagree and only one of them is the content actually present.
        assert_eq!(folds(&documents(patch)[0]), [17]);
    }

    #[test]
    fn a_patch_with_several_files_becomes_several_documents() {
        let patch = "\
diff --git a/one.rs b/one.rs
index 111..222 100644
--- a/one.rs
+++ b/one.rs
@@ -1,1 +1,1 @@
-let a = 1;
+let a = 2;
diff --git a/two.rs b/two.rs
--- a/two.rs
+++ b/two.rs
@@ -1,1 +1,1 @@
-let b = 1;
+let b = 2;
";
        let documents = documents(patch);
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0].new.name, "b/one.rs");
        assert_eq!(documents[1].new.name, "b/two.rs");
    }

    #[test]
    fn noise_outside_a_hunk_is_ignored() {
        // `diff --git`, `index`, and mode lines are not content.
        let patch = "\
diff --git a/x b/x
old mode 100644
new mode 100755
index 1234567..89abcde 100755
--- a/x
+++ b/x
@@ -1,1 +1,1 @@
-a
+b
";
        let document = &documents(patch)[0];
        assert_eq!(document.rows.len(), 1);
    }

    #[test]
    fn a_header_without_counts_still_parses() {
        // `@@ -1 +1 @@` is valid: a count of 1 may be omitted.
        assert_eq!(
            Header::parse("@@ -1 +1 @@"),
            Some(Header {
                old_start: 1,
                new_start: 1
            })
        );
    }

    #[test]
    fn a_header_with_trailing_context_still_parses() {
        assert_eq!(
            Header::parse("@@ -6,5 +6,6 @@ fn render() {"),
            Some(Header {
                old_start: 6,
                new_start: 6
            })
        );
    }

    #[test]
    fn something_that_is_not_a_header_is_not_parsed_as_one() {
        assert_eq!(Header::parse("@@ nonsense"), None);
        assert_eq!(Header::parse(" context @@ -1,1 +1,1 @@"), None);
    }

    #[test]
    fn an_empty_input_produces_nothing() {
        assert!(documents("").is_empty());
    }
}
