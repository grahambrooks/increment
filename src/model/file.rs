//! One side of a comparison, split into lines.

use std::path::Path;

/// How a file terminates its lines.
///
/// Recorded rather than normalised away: a diff that silently treats CRLF as LF
/// will report two identical-looking files as identical when a commit has in
/// fact rewritten every line ending in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
    /// Both appear. Usually a mistake, and worth being able to say so.
    Mixed,
    /// No line terminator anywhere — a single line, or an empty file.
    None,
}

/// One side of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// What to call this side in output. A path, `a/main.rs`, `HEAD:main.rs`.
    pub name: String,
    /// Lines with their terminators stripped. A trailing newline does not
    /// produce a final empty line — `"a\n"` is one line, not two.
    pub lines: Vec<String>,
    /// The last line has content but no terminator.
    pub missing_final_newline: bool,
    pub eol: Eol,
}

impl SourceFile {
    pub fn from_text(name: impl Into<String>, text: &str) -> Self {
        let mut lines = Vec::new();
        let mut saw_lf = false;
        let mut saw_crlf = false;

        let mut rest = text;
        while let Some(index) = rest.find('\n') {
            let (line, tail) = rest.split_at(index);
            if let Some(stripped) = line.strip_suffix('\r') {
                saw_crlf = true;
                lines.push(stripped.to_owned());
            } else {
                saw_lf = true;
                lines.push(line.to_owned());
            }
            rest = &tail[1..];
        }

        // Whatever follows the last newline. Non-empty means the file does not
        // end with one — `\ No newline at end of file` in a unified diff.
        let missing_final_newline = !rest.is_empty();
        if missing_final_newline {
            lines.push(rest.to_owned());
        }

        let eol = match (saw_lf, saw_crlf) {
            (true, true) => Eol::Mixed,
            (true, false) => Eol::Lf,
            (false, true) => Eol::Crlf,
            (false, false) => Eol::None,
        };

        Self {
            name: name.into(),
            lines,
            missing_final_newline,
            eol,
        }
    }

    /// Read a file from disk, naming it by its path.
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        // Lossy rather than an error: a diff tool that refuses to look at a
        // file because one byte is not UTF-8 is less useful than one that shows
        // the file with a replacement character in it.
        let text = String::from_utf8_lossy(&bytes);
        Ok(Self::from_text(path.display().to_string(), &text))
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_newline_does_not_make_a_final_empty_line() {
        let file = SourceFile::from_text("x", "a\nb\n");
        assert_eq!(file.lines, ["a", "b"]);
        assert!(!file.missing_final_newline);
        assert_eq!(file.eol, Eol::Lf);
    }

    #[test]
    fn a_missing_final_newline_is_recorded_not_invented() {
        let file = SourceFile::from_text("x", "a\nb");
        assert_eq!(file.lines, ["a", "b"]);
        assert!(file.missing_final_newline);
    }

    #[test]
    fn crlf_is_stripped_but_remembered() {
        let file = SourceFile::from_text("x", "a\r\nb\r\n");
        assert_eq!(file.lines, ["a", "b"]);
        assert_eq!(file.eol, Eol::Crlf);
    }

    #[test]
    fn mixed_endings_are_reported_as_mixed() {
        assert_eq!(SourceFile::from_text("x", "a\r\nb\n").eol, Eol::Mixed);
    }

    #[test]
    fn an_empty_file_has_no_lines() {
        let file = SourceFile::from_text("x", "");
        assert!(file.is_empty());
        assert!(!file.missing_final_newline);
        assert_eq!(file.eol, Eol::None);
    }

    #[test]
    fn a_blank_line_is_a_line() {
        assert_eq!(SourceFile::from_text("x", "a\n\nb\n").lines, ["a", "", "b"]);
    }
}
