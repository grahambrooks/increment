//! Collapsing unchanged regions.
//!
//! Suppressing the unchanged is the difference between a diff you read and a
//! file you scroll. Unlike `-U3`, the hidden lines are *counted* — a fold row
//! says how many it stands for, so nothing disappears silently, and a later
//! interactive surface can expand it.

use crate::model::{Row, RowKind};

/// Replace runs of equal rows longer than `2 * context + 1` with a fold.
///
/// `None` means no folding: show every row. Leading and trailing runs are
/// trimmed to `context` as well, so a one-line change in a 5000-line file is
/// seven rows rather than five thousand.
pub fn fold(rows: Vec<Row>, context: Option<usize>) -> Vec<Row> {
    let Some(context) = context else {
        return rows;
    };

    // Nothing changed at all: the answer is "no differences", and padding it
    // with context lines of a file that did not change would be noise.
    if !rows.iter().any(Row::is_change) {
        return Vec::new();
    }

    let mut folded: Vec<Row> = Vec::with_capacity(rows.len());
    let mut run: Vec<Row> = Vec::new();

    for row in rows {
        if matches!(row.kind, RowKind::Equal) {
            run.push(row);
            continue;
        }
        flush(&mut folded, std::mem::take(&mut run), context, false);
        folded.push(row);
    }
    flush(&mut folded, run, context, true);

    folded
}

/// Emit a run of equal rows, folding its middle if it is long enough to be
/// worth folding.
fn flush(out: &mut Vec<Row>, run: Vec<Row>, context: usize, trailing: bool) {
    let leading = out.is_empty();

    // How many of this run to keep at each end. A run at the very start has
    // nothing before it to give context to, and likewise at the end.
    let head = if leading { 0 } else { context };
    let tail = if trailing { 0 } else { context };

    // Folding one row away costs a row to say so. Not a saving, and it hides a
    // line for nothing.
    if run.len() <= head + tail + 1 {
        out.extend(run);
        return;
    }

    let hidden = run.len() - head - tail;
    out.extend(run.iter().take(head).cloned());
    out.push(Row::fold(hidden));
    out.extend(run.iter().skip(run.len() - tail).cloned());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Line;

    fn equal(n: usize) -> Row {
        Row::equal(Line::new(n, "same"), Line::new(n, "same"))
    }

    fn changed(n: usize) -> Row {
        Row::added(Line::new(n, "new"))
    }

    fn hidden_counts(rows: &[Row]) -> Vec<usize> {
        rows.iter()
            .filter_map(|row| match row.kind {
                RowKind::Fold { hidden } => Some(hidden),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn no_context_means_no_folding() {
        let rows: Vec<Row> = (1..=20).map(equal).chain([changed(21)]).collect();
        assert_eq!(fold(rows.clone(), None), rows);
    }

    #[test]
    fn an_unchanged_file_folds_to_nothing() {
        let rows: Vec<Row> = (1..=20).map(equal).collect();
        assert!(fold(rows, Some(3)).is_empty());
    }

    #[test]
    fn a_long_leading_run_keeps_only_the_trailing_context() {
        let mut rows: Vec<Row> = (1..=20).map(equal).collect();
        rows.push(changed(21));

        let folded = fold(rows, Some(3));
        // fold + 3 context + the change
        assert_eq!(folded.len(), 5);
        assert_eq!(hidden_counts(&folded), [17]);
        assert!(matches!(folded[0].kind, RowKind::Fold { .. }));
    }

    #[test]
    fn a_long_trailing_run_keeps_only_the_leading_context() {
        let mut rows = vec![changed(1)];
        rows.extend((2..=20).map(equal));

        let folded = fold(rows, Some(3));
        assert_eq!(folded.len(), 5);
        assert_eq!(hidden_counts(&folded), [16]);
        assert!(matches!(folded.last().unwrap().kind, RowKind::Fold { .. }));
    }

    #[test]
    fn a_run_between_two_changes_keeps_context_on_both_sides() {
        let mut rows = vec![changed(1)];
        rows.extend((2..=20).map(equal));
        rows.push(changed(21));

        let folded = fold(rows, Some(3));
        assert_eq!(hidden_counts(&folded), [13]);
        assert_eq!(folded.len(), 1 + 3 + 1 + 3 + 1);
    }

    #[test]
    fn a_short_run_is_left_alone() {
        // Six equal rows between two changes, context 3: folding would hide
        // nothing and cost a row to say so.
        let mut rows = vec![changed(1)];
        rows.extend((2..=7).map(equal));
        rows.push(changed(8));

        let folded = fold(rows.clone(), Some(3));
        assert!(hidden_counts(&folded).is_empty(), "{folded:#?}");
        assert_eq!(folded.len(), rows.len());
    }

    #[test]
    fn folding_accounts_for_every_row_it_hides() {
        let mut rows = vec![changed(1)];
        rows.extend((2..=50).map(equal));
        rows.push(changed(51));
        let total = rows.len();

        let folded = fold(rows, Some(2));
        let shown = folded
            .iter()
            .filter(|row| !matches!(row.kind, RowKind::Fold { .. }))
            .count();
        assert_eq!(shown + hidden_counts(&folded).iter().sum::<usize>(), total);
    }
}
