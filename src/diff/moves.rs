//! Recognising a block that moved.
//!
//! A function lifted from one part of a file to another is *one* change. A
//! line-based diff reports it as a wall of deletions and an equal wall of
//! additions, which is both the largest thing in the diff and the least
//! interesting. Marking it as a move is what lets a reader skip it and find the
//! change that actually matters.
//!
//! The approach is git's `--color-moved`: look for runs of removed lines whose
//! content reappears, in order, among the added lines. Two deliberate limits:
//!
//! - **A run must be substantial.** Three lines, and enough non-whitespace to
//!   be worth the claim. Without a floor, every `}` in the file "moves", and
//!   the marking is noise that hides the real moves.
//! - **Within one file only.** A function moved to a *different* file is not
//!   detected, because each comparison is diffed on its own. Saying so is
//!   better than implying a coverage that is not there; catching it would mean
//!   a pass over every file in the change set before any of them is aligned.

use std::collections::HashMap;

use crate::model::SourceFile;

use super::engine::Block;

/// The shortest run worth calling a move.
const MIN_LINES: usize = 3;

/// …and how much actual content it has to carry between those lines.
///
/// Three closing braces are three lines and mean nothing. This is what stops
/// structural punctuation from being reported as a refactor.
const MIN_SIGNIFICANT_CHARS: usize = 20;

/// Which lines moved, and with which block.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Moves {
    /// Old-side line index to group.
    pub old: HashMap<usize, usize>,
    /// New-side line index to group.
    pub new: HashMap<usize, usize>,
}

impl Moves {
    pub fn is_empty(&self) -> bool {
        self.old.is_empty() && self.new.is_empty()
    }
}

/// Find blocks that moved rather than being deleted and added.
pub fn detect(old: &SourceFile, new: &SourceFile, blocks: &[Block]) -> Moves {
    // Only unmatched lines can have moved; anything the line diff already
    // considers equal stayed where it was.
    let removed: Vec<usize> = blocks.iter().flat_map(|block| block.old.clone()).collect();
    let added: Vec<usize> = blocks.iter().flat_map(|block| block.new.clone()).collect();
    if removed.len() < MIN_LINES || added.len() < MIN_LINES {
        return Moves::default();
    }

    // Where each piece of added content sits, so a candidate run can be found
    // without scanning the whole side for every line.
    let mut by_content: HashMap<&str, Vec<usize>> = HashMap::new();
    for (position, &index) in added.iter().enumerate() {
        by_content
            .entry(new.lines[index].as_str())
            .or_default()
            .push(position);
    }

    let mut moves = Moves::default();
    let mut group = 0usize;
    let mut taken_added = vec![false; added.len()];
    let mut at = 0usize;

    while at < removed.len() {
        let line = old.lines[removed[at]].as_str();
        let candidates = by_content.get(line).cloned().unwrap_or_default();

        // The longest run wins: a short match inside a long one would split a
        // single move into several, and report the same code as moving twice.
        let mut best: Option<(usize, usize)> = None;
        for start in candidates {
            if taken_added[start] {
                continue;
            }
            let mut length = 0usize;
            while at + length < removed.len()
                && start + length < added.len()
                && !taken_added[start + length]
                && old.lines[removed[at + length]] == new.lines[added[start + length]]
            {
                length += 1;
            }
            if best.is_none_or(|(_, best_length)| length > best_length) {
                best = Some((start, length));
            }
        }

        let Some((start, length)) = best else {
            at += 1;
            continue;
        };

        if !worth_reporting(old, &removed[at..at + length]) {
            at += 1;
            continue;
        }

        for offset in 0..length {
            moves.old.insert(removed[at + offset], group);
            moves.new.insert(added[start + offset], group);
            taken_added[start + offset] = true;
        }
        group += 1;
        at += length;
    }

    moves
}

/// Whether a run of lines is substantial enough to call a move.
fn worth_reporting(old: &SourceFile, indices: &[usize]) -> bool {
    if indices.len() < MIN_LINES {
        return false;
    }
    let significant: usize = indices
        .iter()
        .map(|&index| {
            old.lines[index]
                .chars()
                .filter(|c| !c.is_whitespace())
                .count()
        })
        .sum();
    significant >= MIN_SIGNIFICANT_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::engine::{Algorithm, Whitespace, blocks};

    fn detect_in(old: &str, new: &str) -> (SourceFile, SourceFile, Moves) {
        let old = SourceFile::from_text("old", old);
        let new = SourceFile::from_text("new", new);
        let blocks = blocks(
            &old.lines,
            &new.lines,
            Algorithm::Histogram,
            Whitespace::Respect,
        );
        let moves = detect(&old, &new, &blocks);
        (old, new, moves)
    }

    const FUNCTION: &str = "\
fn helper(value: u32) -> u32 {
    let doubled = value * 2;
    doubled + 1
}
";

    const CALLER: &str = "\
fn main() {
    let answer = helper(1);
    println!(\"{answer}\");
}
";

    #[test]
    fn a_function_moved_within_a_file_is_a_move() {
        let old = format!("{FUNCTION}{CALLER}");
        let new = format!("{CALLER}{FUNCTION}");

        let (old_file, new_file, moves) = detect_in(&old, &new);
        assert!(!moves.is_empty(), "the swap should read as a move");

        // Which of the two blocks the line diff considers to have moved is its
        // call to make — one copy has to be the anchor, and either reading is
        // true. What must hold is that a whole block moved as one, and that
        // both sides agree on it.
        assert_eq!(moves.old.len(), moves.new.len());
        assert_eq!(moves.old.len(), 4, "a whole four-line block: {moves:?}");
        assert_eq!(
            moves
                .old
                .values()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            1,
            "one move, not several"
        );

        let moved_old: Vec<&str> = sorted_lines(&old_file, &moves.old);
        let moved_new: Vec<&str> = sorted_lines(&new_file, &moves.new);
        assert_eq!(moved_old, moved_new, "the same lines on both sides");
    }

    fn sorted_lines<'a>(file: &'a SourceFile, indices: &HashMap<usize, usize>) -> Vec<&'a str> {
        let mut keys: Vec<usize> = indices.keys().copied().collect();
        keys.sort_unstable();
        keys.iter().map(|&i| file.lines[i].as_str()).collect()
    }

    #[test]
    fn code_that_did_not_move_is_not_marked() {
        let (_, _, moves) = detect_in("let a = 1;\nlet b = 2;\n", "let a = 1;\nlet b = 3;\n");
        assert!(moves.is_empty(), "{moves:?}");
    }

    #[test]
    fn structural_punctuation_alone_does_not_move() {
        // Braces and blank lines appear everywhere. Without a floor on what
        // counts, they "move" constantly, and the marking becomes noise that
        // buries the moves worth seeing.
        let old = "fn a() {\n}\n\nfn b() {\n}\n";
        let new = "fn b() {\n}\n\nfn a() {\n}\n";
        let (old_file, _, moves) = detect_in(old, new);

        for &index in moves.old.keys() {
            let text = old_file.lines[index].trim();
            assert!(
                text.len() > 1,
                "line {index} ({text:?}) is too trivial to call a move"
            );
        }
    }

    #[test]
    fn a_short_run_is_below_the_threshold() {
        let old = "keep\nx()\ny()\nkeep2\n";
        let new = "keep\nkeep2\nx()\ny()\n";
        let (_, _, moves) = detect_in(old, new);
        assert!(
            moves.is_empty(),
            "two short lines are not a move: {moves:?}"
        );
    }

    #[test]
    fn a_moved_block_that_was_also_edited_is_not_claimed_whole() {
        // Only lines that survived unchanged can be matched by content, so an
        // edited line inside a moved block is not part of the move. Claiming it
        // would hide a real change inside something the reader is told to skip.
        let old = format!("{FUNCTION}\nfn main() {{}}\n");
        let new = "fn main() {}\n\nfn helper(value: u32) -> u32 {\n    let doubled = value * 3;\n    doubled + 1\n}\n";

        let (_, new_file, moves) = detect_in(&old, new);
        let moved_new: Vec<&str> = moves
            .new
            .keys()
            .map(|&index| new_file.lines[index].as_str())
            .collect();
        assert!(
            !moved_new.iter().any(|line| line.contains("value * 3")),
            "the edited line was claimed as moved: {moved_new:?}"
        );
    }

    #[test]
    fn two_blocks_that_moved_separately_are_separate_groups() {
        // One tint for both would read as a single block, which is the thing
        // alternating tints exist to prevent. An unchanged middle keeps the two
        // moves from being one contiguous run.
        let a = "fn alpha() -> u32 {\n    let first = compute_one();\n    first + 1\n}\n";
        let b = "fn beta() -> u32 {\n    let second = compute_two();\n    second + 2\n}\n";
        let middle: String = (1..=10)
            .map(|n| format!("const MIDDLE_{n}: u32 = {n};\n"))
            .collect();

        let old = format!("{a}{middle}{b}");
        let new = format!("{b}{middle}{a}");

        let (_, _, moves) = detect_in(&old, &new);
        let groups: std::collections::HashSet<_> = moves.old.values().collect();
        assert!(
            groups.len() >= 2,
            "expected two distinct moves, got {groups:?} from {moves:?}"
        );
    }
}
