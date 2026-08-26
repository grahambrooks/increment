//! Tokenizing, for both levels of the diff.
//!
//! One engine serves the outer (line) and inner (word) diffs; only the token
//! stream differs. Words come from unicode word boundaries rather than
//! `split_whitespace`, so punctuation, identifiers and CJK each land in their
//! own token and the emphasis lands on `foo` rather than on `foo(bar,`.

use unicode_segmentation::UnicodeSegmentation;

/// A word of a line, with where it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Word<'a> {
    pub text: &'a str,
    /// Byte offset of `text` within the line it came from.
    pub start: usize,
}

impl Word<'_> {
    pub fn end(&self) -> usize {
        self.start + self.text.len()
    }

    /// Whitespace is a token like any other for diffing purposes, but it is not
    /// worth *emphasising* on its own, and it should not count towards how
    /// similar two lines are — otherwise two unrelated lines at the same indent
    /// look alike.
    pub fn is_whitespace(&self) -> bool {
        self.text.chars().all(char::is_whitespace)
    }
}

/// Split a line at unicode word boundaries, keeping byte offsets.
pub fn words(line: &str) -> Vec<Word<'_>> {
    line.split_word_bound_indices()
        .map(|(start, text)| Word { text, start })
        .collect()
}

/// How alike two lines are, in `0.0..=1.0`.
///
/// A Dice coefficient over word multisets, **weighted by word length**: twice
/// the shared length over the total. Cheap — no edit script — which matters
/// because alignment asks this question far more often than it accepts an
/// answer.
///
/// Two exclusions, both learned from wrong answers this produced unweighted:
/// whitespace does not count, or indentation alone makes two lines look alike;
/// and length weighting is what stops `(`, `)` and `;` — which every line of
/// code shares — from carrying the same weight as an identifier. Unweighted,
/// `let b = 2;` and `let inserted = 0;` score 0.6 and pair, which produces a
/// confident and completely wrong set of inline highlights.
pub fn similarity(left: &str, right: &str) -> f32 {
    if left == right {
        return 1.0;
    }

    let significant = |line: &str| -> Vec<String> {
        let mut words: Vec<String> = words(line)
            .into_iter()
            .filter(|word| !word.is_whitespace())
            .map(|word| word.text.to_owned())
            .collect();
        words.sort_unstable();
        words
    };

    let left = significant(left);
    let right = significant(right);
    if left.is_empty() && right.is_empty() {
        // Two blank-but-different lines: different whitespace only.
        return 1.0;
    }
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let weight =
        |words: &[String]| -> usize { words.iter().map(|word| word.chars().count()).sum() };
    let (left_weight, right_weight) = (weight(&left), weight(&right));

    // Both sorted, so the shared weight is a merge.
    let (mut i, mut j, mut shared) = (0, 0, 0usize);
    while i < left.len() && j < right.len() {
        match left[i].cmp(&right[j]) {
            std::cmp::Ordering::Equal => {
                shared += left[i].chars().count();
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }

    (2 * shared) as f32 / (left_weight + right_weight) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_carry_their_offsets() {
        let line = "let x = 1;";
        let words = words(line);
        for word in &words {
            assert_eq!(&line[word.start..word.end()], word.text);
        }
        let significant: Vec<_> = words
            .iter()
            .filter(|word| !word.is_whitespace())
            .map(|word| word.text)
            .collect();
        assert_eq!(significant, ["let", "x", "=", "1", ";"]);
    }

    #[test]
    fn punctuation_is_its_own_token() {
        let words = words("foo(bar,");
        let texts: Vec<_> = words.iter().map(|word| word.text).collect();
        assert_eq!(texts, ["foo", "(", "bar", ","]);
    }

    #[test]
    fn identical_lines_are_perfectly_similar() {
        assert_eq!(similarity("let x = 1;", "let x = 1;"), 1.0);
    }

    #[test]
    fn an_edited_line_stays_similar() {
        assert!(similarity("let x = 1;", "let x = 2;") > 0.5);
    }

    #[test]
    fn unrelated_lines_are_not_similar() {
        assert!(similarity("let x = 1;", "impl Display for Widget {") < 0.5);
    }

    #[test]
    fn shared_punctuation_alone_does_not_make_lines_similar() {
        // Every line of code ends in `;` and most contain brackets. Counting
        // each as one word, equal in weight to an identifier, pairs lines that
        // have nothing to do with each other.
        let score = similarity("let b = 2;", "let inserted = 0;");
        assert!(score < 0.5, "{score}");
    }

    #[test]
    fn indentation_alone_does_not_make_lines_similar() {
        // Both are deeply indented and share nothing else. Counting whitespace
        // as a word would score these as a near match and produce a nonsense
        // pairing with nonsense inline highlights.
        let left = "            return Ok(());";
        let right = "            self.buffer.clear();";
        assert!(similarity(left, right) < 0.5, "{}", similarity(left, right));
    }
}
