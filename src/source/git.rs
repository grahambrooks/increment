//! Git, via `gix`, in pure Rust.
//!
//! The point of reading git natively rather than parsing `git diff`'s output is
//! *fidelity*: both sides arrive in full, so folding, whole-file context and
//! re-diffing all work. A pager only ever sees the context git chose to print
//! — see [`super::patch`] for what that costs.
//!
//! Three shapes, matching what `git diff` accepts:
//!
//! - nothing — `HEAD` against the working tree
//! - a revision — that revision against the working tree
//! - `a..b` — one revision against another

use std::collections::BTreeMap;
use std::path::PathBuf;

use gix::bstr::ByteSlice;

use crate::model::SourceFile;

use super::{Changes, Comparison, is_binary};

#[derive(Debug)]
pub enum Error {
    NotARepository(String),
    Revision { spec: String, reason: String },
    Read(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotARepository(reason) => write!(f, "not a git repository: {reason}"),
            Self::Revision { spec, reason } => write!(f, "bad revision {spec:?}: {reason}"),
            Self::Read(reason) => write!(f, "reading from git: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

/// One commit, as the review list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Abbreviated, for display.
    pub short_id: String,
    /// Full, for asking for its diff later.
    pub id: String,
    pub summary: String,
    pub author: String,
    /// `YYYY-MM-DD`, which sorts and aligns.
    pub date: String,
}

/// Walk the history named by a `git log`-style spec.
///
/// `a..b` walks `b` and stops at `a`; a bare revision walks from there; nothing
/// walks from `HEAD`. Newest first, as `git log` does — a review starts at the
/// top of the branch.
pub fn log(spec: Option<&str>, limit: Option<usize>) -> Result<Vec<Commit>, Error> {
    let repo = gix::discover(".").map_err(|error| Error::NotARepository(error.to_string()))?;

    let (tip, hidden) = match spec.and_then(split_range) {
        Some((from, to)) => (to.to_owned(), Some(from.to_owned())),
        None => (spec.unwrap_or("HEAD").to_owned(), None),
    };

    let tip_id = repo
        .rev_parse_single(tip.as_str())
        .map_err(|error| Error::Revision {
            spec: tip.clone(),
            reason: error.to_string(),
        })?
        .detach();

    let mut walk = repo.rev_walk([tip_id]);
    if let Some(hidden) = &hidden {
        // The exclusive end of the range: `a..b` is everything reachable from
        // `b` that is not reachable from `a`.
        let hidden_id = repo
            .rev_parse_single(hidden.as_str())
            .map_err(|error| Error::Revision {
                spec: hidden.clone(),
                reason: error.to_string(),
            })?
            .detach();
        walk = walk.with_hidden([hidden_id]);
    }

    let walk = walk.all().map_err(|error| Error::Read(error.to_string()))?;

    let mut commits = Vec::new();
    for info in walk {
        if limit.is_some_and(|limit| commits.len() >= limit) {
            break;
        }
        let info = info.map_err(|error| Error::Read(error.to_string()))?;
        let object = info
            .object()
            .map_err(|error| Error::Read(error.to_string()))?;

        // A commit whose fields will not decode is still a commit. Showing it
        // with what could be read beats dropping it from the history.
        let summary = object
            .message()
            .map(|message| message.summary().to_string())
            .unwrap_or_else(|_| "<unreadable message>".to_owned());
        let author = object
            .author()
            .map(|author| author.name.to_string())
            .unwrap_or_default();
        // Just the date: a review list is scanned down the left edge, and a
        // full timestamp costs eleven columns to say what the summary already
        // implies.
        // The date only. gix's ISO8601 is `2026-08-26 16:52:45 -0600`, and the
        // time costs fourteen columns of a list that is scanned down its left
        // edge — columns the summary needs more.
        let date = object
            .time()
            .ok()
            .and_then(|time| time.format(gix::date::time::format::ISO8601).ok())
            .map(|text| text.split_whitespace().next().unwrap_or(&text).to_owned())
            .unwrap_or_default();

        commits.push(Commit {
            short_id: object
                .short_id()
                .map(|id| id.to_string())
                .unwrap_or_else(|_| info.id.to_string()),
            id: info.id.to_string(),
            summary,
            author,
            date,
        });
    }

    Ok(commits)
}

/// The diff a single commit introduced: its first parent against itself.
pub fn commit(id: &str, paths: &[PathBuf]) -> Result<Changes, Error> {
    compare(Some(&format!("{id}~1..{id}")), paths).or_else(|error| {
        // A root commit has no parent, so `id~1` does not resolve. Everything
        // in it is an addition.
        match error {
            Error::Revision { .. } => compare(Some(&format!("..{id}")), paths),
            other => Err(other),
        }
    })
}

/// Compare, according to a `git diff`-style revision spec.
///
/// `paths`, if given, restricts the result to entries at or below one of them.
pub fn compare(spec: Option<&str>, paths: &[PathBuf]) -> Result<Changes, Error> {
    let repo = gix::discover(".").map_err(|error| Error::NotARepository(error.to_string()))?;

    match spec.and_then(split_range) {
        Some((old, new)) => tree_to_tree(&repo, old, new, paths),
        None => tree_to_worktree(&repo, spec.unwrap_or("HEAD"), paths),
    }
}

/// Split `a..b` (or `a...b`) into its two revisions.
///
/// `...` is accepted and treated as `..`: symmetric difference is a *commit
/// selection* concept, and this tool compares two states rather than walking
/// history, so silently doing something different would be worse than being
/// slightly generous here.
fn split_range(spec: &str) -> Option<(&str, &str)> {
    let (old, new) = spec.split_once("...").or_else(|| spec.split_once(".."))?;
    Some((
        if old.is_empty() { "HEAD" } else { old },
        if new.is_empty() { "HEAD" } else { new },
    ))
}

fn tree_to_tree(
    repo: &gix::Repository,
    old_spec: &str,
    new_spec: &str,
    paths: &[PathBuf],
) -> Result<Changes, Error> {
    let old_tree = tree(repo, old_spec)?;
    let new_tree = tree(repo, new_spec)?;

    let changes = repo
        .diff_tree_to_tree(&old_tree, &new_tree, None)
        .map_err(|error| Error::Read(error.to_string()))?;

    let mut out = Changes::default();
    for change in changes {
        use gix::object::tree::diff::ChangeDetached as Change;
        let (location, old_id, new_id) = match change {
            Change::Addition {
                location,
                id,
                entry_mode,
                ..
            } => {
                if !entry_mode.is_blob() {
                    continue;
                }
                (location, None, Some(id))
            }
            Change::Deletion {
                location,
                id,
                entry_mode,
                ..
            } => {
                if !entry_mode.is_blob() {
                    continue;
                }
                (location, Some(id), None)
            }
            Change::Modification {
                location,
                previous_id,
                id,
                entry_mode,
                ..
            } => {
                if !entry_mode.is_blob() {
                    continue;
                }
                (location, Some(previous_id), Some(id))
            }
            // A rename is one change, but it is still two contents to show.
            Change::Rewrite {
                source_location,
                source_id,
                location,
                id,
                ..
            } => {
                let path = location.to_str_lossy().into_owned();
                if !selected(&path, paths) {
                    continue;
                }
                push(
                    &mut out,
                    &format!("a/{}", source_location.to_str_lossy()),
                    &format!("b/{path}"),
                    blob(repo, Some(source_id))?,
                    blob(repo, Some(id))?,
                );
                continue;
            }
        };

        let path = location.to_str_lossy().into_owned();
        if !selected(&path, paths) {
            continue;
        }
        push(
            &mut out,
            &format!("a/{path}"),
            &format!("b/{path}"),
            blob(repo, old_id)?,
            blob(repo, new_id)?,
        );
    }

    Ok(out)
}

/// A revision against what is on disk right now.
///
/// Built from the index rather than by hashing every tracked file: an entry
/// whose stat still matches the index is known to hold the content the index
/// records, so the comparison is two object ids and no read at all. Only files
/// that actually look touched are read. This is git's own rule, and the reason
/// `git status` is fast on a large checkout.
fn tree_to_worktree(
    repo: &gix::Repository,
    spec: &str,
    paths: &[PathBuf],
) -> Result<Changes, Error> {
    let tree = tree(repo, spec)?;
    let tracked = blobs_in(repo, &tree)?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| Error::NotARepository("this repository has no working tree".to_owned()))?
        .to_owned();

    let index = repo
        .index_or_empty()
        .map_err(|error| Error::Read(error.to_string()))?;
    let stat_options = repo
        .stat_options()
        .map_err(|error| Error::Read(error.to_string()))?;

    let mut out = Changes::default();
    let mut seen: Vec<String> = Vec::new();

    for entry in index.entries() {
        let path = entry.path(&index).to_str_lossy().into_owned();
        if !selected(&path, paths) {
            continue;
        }
        seen.push(path.clone());

        let on_disk = workdir.join(&path);
        let old_id = tracked.get(&path).copied();

        let new_bytes =
            match gix::index::fs::Metadata::from_path_no_follow(&on_disk) {
                // Tracked but gone from disk: a deletion, whatever the index says.
                Err(_) => None,
                Ok(metadata) => {
                    let stat = gix::index::entry::Stat::from_fs(&metadata).ok();

                    // Trusting the stat needs two things: that it matches what the
                    // index recorded, and that the entry is not *racy* — written in
                    // the same second the index was, where a same-size edit is
                    // indistinguishable from no edit at all. Git has the same
                    // problem and solves it the same way: when in doubt, read.
                    //
                    // This is not a corner case. Committing and immediately editing
                    // a line to the same length is an ordinary thing to do, and
                    // without this check the change is silently invisible.
                    let trust_stat = stat.is_some_and(|stat| {
                        stat.matches(&entry.stat, stat_options)
                            && !entry.stat.is_racy(index.timestamp(), stat_options)
                    });

                    if trust_stat {
                        // The file holds what the index says it holds. If the index
                        // also matches the tree, nothing changed and nothing is read.
                        if old_id == Some(entry.id) {
                            continue;
                        }
                        Some(read_blob(repo, entry.id)?)
                    } else {
                        Some(std::fs::read(&on_disk).map_err(|error| {
                            Error::Read(format!("{}: {error}", on_disk.display()))
                        })?)
                    }
                }
            };

        let old_bytes = old_id.map(|id| read_blob(repo, id)).transpose()?;
        if old_bytes == new_bytes {
            continue;
        }

        push(
            &mut out,
            &format!("a/{path}"),
            &format!("b/{path}"),
            old_bytes,
            new_bytes,
        );
    }

    // In the tree but not the index: staged deletions.
    for (path, id) in &tracked {
        if seen.contains(path) || !selected(path, paths) {
            continue;
        }
        let old_bytes = read_blob(repo, *id)?;
        push(
            &mut out,
            &format!("a/{path}"),
            &format!("b/{path}"),
            Some(old_bytes),
            None,
        );
    }

    out.comparisons.sort_by(|a, b| a.new.name.cmp(&b.new.name));
    Ok(out)
}

/// Every blob in a tree, by path.
fn blobs_in(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
) -> Result<BTreeMap<String, gix::ObjectId>, Error> {
    let mut blobs = BTreeMap::new();
    let mut recorder = gix::traverse::tree::Recorder::default();
    tree.traverse()
        .breadthfirst(&mut recorder)
        .map_err(|error| Error::Read(error.to_string()))?;

    for entry in recorder.records {
        if entry.mode.is_blob() {
            blobs.insert(entry.filepath.to_str_lossy().into_owned(), entry.oid);
        }
    }
    let _ = repo;
    Ok(blobs)
}

fn tree<'repo>(repo: &'repo gix::Repository, spec: &str) -> Result<gix::Tree<'repo>, Error> {
    let bad = |error: &dyn std::fmt::Display| Error::Revision {
        spec: spec.to_owned(),
        reason: error.to_string(),
    };

    repo.rev_parse_single(spec)
        .map_err(|error| bad(&error))?
        .object()
        .map_err(|error| bad(&error))?
        .peel_to_tree()
        .map_err(|error| bad(&error))
}

fn blob(repo: &gix::Repository, id: Option<gix::ObjectId>) -> Result<Option<Vec<u8>>, Error> {
    id.map(|id| read_blob(repo, id)).transpose()
}

fn read_blob(repo: &gix::Repository, id: gix::ObjectId) -> Result<Vec<u8>, Error> {
    repo.find_object(id)
        .map(|object| object.detach().data)
        .map_err(|error| Error::Read(error.to_string()))
}

/// Record one changed path, or note it as binary.
///
/// A missing side is an empty file rather than an absent one: an addition is a
/// diff against nothing, and modelling it that way means every renderer shows
/// it without a special case.
fn push(
    out: &mut Changes,
    old_name: &str,
    new_name: &str,
    old_bytes: Option<Vec<u8>>,
    new_bytes: Option<Vec<u8>>,
) {
    let binary =
        old_bytes.as_deref().is_some_and(is_binary) || new_bytes.as_deref().is_some_and(is_binary);
    if binary {
        out.binary.push(new_name.to_owned());
        return;
    }

    let text = |bytes: Option<Vec<u8>>| {
        bytes.map_or_else(String::new, |bytes| {
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };

    out.comparisons.push(Comparison::new(
        SourceFile::from_text(old_name, &text(old_bytes)),
        SourceFile::from_text(new_name, &text(new_bytes)),
    ));
}

/// Whether a path passes the path filters. No filters means everything.
fn selected(path: &str, paths: &[PathBuf]) -> bool {
    paths.is_empty()
        || paths.iter().any(|filter| {
            let filter = filter.to_string_lossy();
            path == filter || path.starts_with(&format!("{}/", filter.trim_end_matches('/')))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_recognised_and_split() {
        assert_eq!(split_range("HEAD~1..HEAD"), Some(("HEAD~1", "HEAD")));
        assert_eq!(split_range("main..feature"), Some(("main", "feature")));
    }

    #[test]
    fn a_bare_revision_is_not_a_range() {
        assert_eq!(split_range("HEAD"), None);
        assert_eq!(split_range("v1.0"), None);
    }

    #[test]
    fn an_open_ended_range_means_head() {
        assert_eq!(split_range("main.."), Some(("main", "HEAD")));
        assert_eq!(split_range("..main"), Some(("HEAD", "main")));
    }

    #[test]
    fn three_dots_is_accepted_as_a_range() {
        // `...` is a commit-selection concept. This tool compares two states,
        // so it is treated as `..` rather than quietly doing something else.
        assert_eq!(split_range("main...feature"), Some(("main", "feature")));
    }

    #[test]
    fn no_filters_selects_everything() {
        assert!(selected("src/main.rs", &[]));
    }

    #[test]
    fn a_filter_matches_a_file_or_a_directory_prefix() {
        let filters = vec![PathBuf::from("src")];
        assert!(selected("src/main.rs", &filters));
        assert!(selected("src/render/split.rs", &filters));
        assert!(!selected("tests/cli.rs", &filters));
        // Not a prefix match on the raw string: `srcery/` is not `src/`.
        assert!(!selected("srcery/thing.rs", &filters));
    }

    #[test]
    fn a_filter_can_name_one_file() {
        let filters = vec![PathBuf::from("Cargo.toml")];
        assert!(selected("Cargo.toml", &filters));
        assert!(!selected("Cargo.lock", &filters));
    }
}
