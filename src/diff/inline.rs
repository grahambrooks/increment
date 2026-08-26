//! Word-level emphasis inside a paired line.
//!
//! The highest-value feature in every diff tool worth using: show the changed
//! *substring*, not the changed line. The same engine runs here as at the line
//! level — only the token stream changes.

use imara_diff::InternedInput;
use imara_diff::{Algorithm, Diff};

use crate::model::Span;

use super::tokens::{Word, words};

/// The changed regions of a pair of lines: `(left, right)`.
///
/// Spans are byte ranges into their own line, merged where adjacent so a run of
/// changed words highlights as one region rather than a stutter of small ones.
pub fn emphasis(left: &str, right: &str) -> (Vec<Span>, Vec<Span>) {
    if left == right {
        return (Vec::new(), Vec::new());
    }

    let left_words = words(left);
    let right_words = words(right);

    let mut input = InternedInput::default();
    input.update_before(left_words.iter().map(|word| word.text));
    input.update_after(right_words.iter().map(|word| word.text));

    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    // The line-oriented slider heuristic is about indentation, which means
    // nothing within a line.
    diff.postprocess_no_heuristic(&input);

    let mut left_spans = Vec::new();
    let mut right_spans = Vec::new();
    for hunk in diff.hunks() {
        push_span(&mut left_spans, &left_words, &hunk.before);
        push_span(&mut right_spans, &right_words, &hunk.after);
    }

    // If the whole of both sides changed, highlighting every word says less
    // than highlighting nothing: the row colour already carries it.
    if covers_everything(&left_spans, left) && covers_everything(&right_spans, right) {
        return (Vec::new(), Vec::new());
    }

    (merge(left_spans), merge(right_spans))
}

fn push_span(spans: &mut Vec<Span>, words: &[Word<'_>], range: &std::ops::Range<u32>) {
    let (start, end) = (range.start as usize, range.end as usize);
    if start >= end {
        return;
    }
    spans.push(Span::new(words[start].start, words[end - 1].end()));
}

fn covers_everything(spans: &[Span], line: &str) -> bool {
    !line.is_empty() && spans.len() == 1 && spans[0].start == 0 && spans[0].end == line.len()
}

/// Join spans that touch, so `foo` `(` `bar` highlights as one region.
fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    spans.retain(|span| !span.is_empty());
    spans.sort_unstable_by_key(|span| span.start);

    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slices<'a>(line: &'a str, spans: &[Span]) -> Vec<&'a str> {
        spans
            .iter()
            .map(|span| &line[span.start..span.end])
            .collect()
    }

    #[test]
    fn identical_lines_have_no_emphasis() {
        assert_eq!(emphasis("same", "same"), (Vec::new(), Vec::new()));
    }

    #[test]
    fn only_the_changed_word_is_emphasised() {
        let (left, right) = emphasis("let x = 1;", "let x = 2;");
        assert_eq!(slices("let x = 1;", &left), ["1"]);
        assert_eq!(slices("let x = 2;", &right), ["2"]);
    }

    #[test]
    fn an_insertion_emphasises_only_the_new_side() {
        let (left, right) = emphasis("foo(a)", "foo(a, b)");
        assert!(left.is_empty(), "nothing was removed, got {left:?}");
        assert_eq!(slices("foo(a, b)", &right), [", b"]);
    }

    #[test]
    fn adjacent_changes_merge_into_one_region() {
        let (_, right) = emphasis("a = one;", "a = two_thing;");
        assert_eq!(right.len(), 1, "expected one merged span, got {right:?}");
    }

    #[test]
    fn spans_fall_on_utf8_boundaries() {
        let left = "let 名前 = 1;";
        let right = "let 名前 = 2;";
        let (left_spans, right_spans) = emphasis(left, right);
        // Slicing panics if a span splits a multi-byte character.
        let _ = slices(left, &left_spans);
        let _ = slices(right, &right_spans);
    }

    #[test]
    fn a_wholly_different_line_is_not_emphasised_end_to_end() {
        // The row colour already says "this changed"; painting every character
        // adds noise and hides the case where only part of a line moved.
        let (left, right) = emphasis("alpha", "beta");
        assert!(left.is_empty() && right.is_empty(), "{left:?} {right:?}");
    }
}
