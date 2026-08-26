//! Two paths.

use std::path::Path;

use crate::model::SourceFile;

use super::{Changes, Comparison, is_binary};

/// Read two files.
///
/// Naming keeps the paths as given, so the header shows what was typed and the
/// extension is available for syntax detection.
///
/// The error carries the path that actually failed. Attaching the caller's
/// first path instead — which is what a bare `io::Error` leads to — sends the
/// reader to look at the wrong file.
pub fn compare(old: &Path, new: &Path) -> Result<Changes, String> {
    let old_bytes = read(old)?;
    let new_bytes = read(new)?;

    if is_binary(&old_bytes) || is_binary(&new_bytes) {
        return Ok(Changes {
            binary: vec![format!("{} and {}", old.display(), new.display())],
            ..Changes::default()
        });
    }

    Ok(Changes {
        comparisons: vec![Comparison::new(
            SourceFile::from_text(
                old.display().to_string(),
                &String::from_utf8_lossy(&old_bytes),
            ),
            SourceFile::from_text(
                new.display().to_string(),
                &String::from_utf8_lossy(&new_bytes),
            ),
        )],
        ..Changes::default()
    })
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}
