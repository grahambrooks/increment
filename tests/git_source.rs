//! The git source, against real repositories.
//!
//! These build a repository with the `git` binary and then check gdiff against
//! it. Using git as the oracle is the point: the claim worth testing is not
//! "the code runs" but "it selects the same changes git does", and only git can
//! settle that.
//!
//! Note the asymmetry with the product: gdiff itself never shells out to git —
//! it reads the repository with `gix`, in process. Git is a *test* dependency
//! here, and if it is missing these fail loudly rather than skipping quietly.

use std::path::PathBuf;
use std::process::Command;

struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gdiff-git-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let repo = Self { dir };
        repo.git(&["init", "--initial-branch=main"]);
        // Committing needs an identity, and the ambient one may be absent or
        // may be signed, which would make these tests depend on a key.
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "user.name", "gdiff tests"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(&self.dir)
            .args(args)
            .output()
            .unwrap_or_else(|error| {
                panic!("these tests need the `git` binary to build fixtures: {error}")
            });
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn write(&self, path: &str, contents: &str) {
        let full = self.dir.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(full, contents).expect("write");
    }

    fn write_bytes(&self, path: &str, contents: &[u8]) {
        std::fs::write(self.dir.join(path), contents).expect("write");
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-m", message]);
    }

    /// Run gdiff inside the repository.
    fn gdiff(&self, args: &[&str]) -> (String, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_gdiff"))
            .current_dir(&self.dir)
            .args(args)
            .output()
            .expect("the binary runs");
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            output.status.code().unwrap_or(-1),
        )
    }

    /// The files gdiff reports, from its unified headers.
    fn changed_files(&self, args: &[&str]) -> Vec<String> {
        let (stdout, _) = self.gdiff(&[args, &["--view", "unified"]].concat());
        let mut files: Vec<String> = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("+++ b/"))
            .chain(
                stdout
                    .lines()
                    .filter_map(|line| line.strip_prefix("Binary file b/"))
                    .map(|line| line.trim_end_matches(" differs")),
            )
            .map(str::to_owned)
            .collect();
        files.sort();
        files
    }

    /// The files git reports, as the oracle.
    fn git_changed_files(&self, args: &[&str]) -> Vec<String> {
        let mut files: Vec<String> = self
            .git(&[&["diff", "--name-only"], args].concat())
            .lines()
            .map(str::to_owned)
            .collect();
        files.sort();
        files
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn seed(repo: &Repo) {
    repo.write("src/main.rs", "fn main() {\n    println!(\"one\");\n}\n");
    repo.write(
        "src/lib.rs",
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    );
    repo.write("README.md", "# project\n\nA thing.\n");
    repo.commit("initial");
}

#[test]
fn the_working_tree_against_head_matches_what_git_reports() {
    let repo = Repo::new("worktree");
    seed(&repo);
    repo.write("src/main.rs", "fn main() {\n    println!(\"two\");\n}\n");
    repo.write("README.md", "# project\n\nA better thing.\n");

    assert_eq!(
        repo.changed_files(&["git"]),
        repo.git_changed_files(&["HEAD"])
    );
    assert_eq!(repo.changed_files(&["git"]), ["README.md", "src/main.rs"]);
}

/// The racy-index case, pinned deliberately.
///
/// A file edited in the same second the index was written, to exactly the same
/// length, is indistinguishable from an untouched one by stat alone. Trusting
/// the stat there makes the change silently invisible — no error, no output,
/// exit 0, as though the file were clean. Git reads the file when an entry
/// falls in that window and so must this.
#[test]
fn a_same_second_same_length_edit_is_still_found() {
    let repo = Repo::new("racy");
    repo.write("f.rs", "let value = \"one\";\n");
    repo.commit("initial");
    // Same byte length, immediately after the commit.
    repo.write("f.rs", "let value = \"two\";\n");

    assert_eq!(repo.changed_files(&["git"]), ["f.rs"]);
    assert_eq!(
        repo.changed_files(&["git"]),
        repo.git_changed_files(&["HEAD"])
    );
}

#[test]
fn an_unmodified_tree_reports_nothing_and_exits_zero() {
    let repo = Repo::new("clean");
    seed(&repo);

    let (stdout, code) = repo.gdiff(&["git"]);
    assert_eq!(code, 0, "exit code");
    assert!(stdout.is_empty(), "{stdout}");
}

#[test]
fn a_range_between_two_commits_matches_what_git_reports() {
    let repo = Repo::new("range");
    seed(&repo);
    repo.write(
        "src/lib.rs",
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b + 0\n}\n",
    );
    repo.write("src/new.rs", "pub fn extra() {}\n");
    repo.commit("second");

    assert_eq!(
        repo.changed_files(&["git", "HEAD~1..HEAD"]),
        repo.git_changed_files(&["HEAD~1..HEAD"])
    );
    assert_eq!(
        repo.changed_files(&["git", "HEAD~1..HEAD"]),
        ["src/lib.rs", "src/new.rs"]
    );
}

#[test]
fn a_revision_on_its_own_compares_it_against_the_working_tree() {
    let repo = Repo::new("rev");
    seed(&repo);
    repo.write(
        "src/lib.rs",
        "pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n",
    );
    repo.commit("second");
    repo.write("README.md", "# project\n\nEdited on disk.\n");

    // HEAD~1 against the working tree: both the committed change and the
    // uncommitted one.
    assert_eq!(
        repo.changed_files(&["git", "HEAD~1"]),
        repo.git_changed_files(&["HEAD~1"])
    );
    assert_eq!(
        repo.changed_files(&["git", "HEAD~1"]),
        ["README.md", "src/lib.rs"]
    );
}

#[test]
fn path_filters_narrow_the_result() {
    let repo = Repo::new("paths");
    seed(&repo);
    repo.write("src/main.rs", "fn main() {\n    println!(\"two\");\n}\n");
    repo.write("README.md", "# project\n\nChanged.\n");

    assert_eq!(repo.changed_files(&["git", "HEAD", "src"]), ["src/main.rs"]);
    assert_eq!(
        repo.changed_files(&["git", "HEAD", "README.md"]),
        ["README.md"]
    );
}

#[test]
fn a_deleted_file_is_a_diff_against_nothing() {
    let repo = Repo::new("deleted");
    seed(&repo);
    std::fs::remove_file(repo.dir.join("README.md")).expect("remove");

    assert_eq!(repo.changed_files(&["git"]), ["README.md"]);

    let (stdout, code) = repo.gdiff(&["git", "--view", "unified"]);
    assert_eq!(code, 1, "exit code");
    assert!(stdout.contains("-# project"), "{stdout}");
}

#[test]
fn a_new_file_is_shown_as_all_additions() {
    let repo = Repo::new("added");
    seed(&repo);
    repo.write("src/added.rs", "pub fn brand_new() {}\n");
    // Staged, because an untracked file is not part of `git diff` either.
    repo.git(&["add", "src/added.rs"]);

    assert_eq!(
        repo.changed_files(&["git"]),
        repo.git_changed_files(&["HEAD"])
    );
    let (stdout, _) = repo.gdiff(&["git", "--view", "unified"]);
    assert!(stdout.contains("+pub fn brand_new() {}"), "{stdout}");
}

#[test]
fn a_binary_file_is_named_rather_than_rendered_or_dropped() {
    let repo = Repo::new("binary");
    seed(&repo);
    repo.write_bytes("logo.png", b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR");
    repo.git(&["add", "logo.png"]);
    repo.commit("add a binary");
    repo.write_bytes("logo.png", b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0eIHDR!");

    let (stdout, code) = repo.gdiff(&["git", "--view", "unified"]);
    assert_eq!(code, 1, "a binary change is still a change");
    assert!(
        stdout.contains("Binary file b/logo.png differs"),
        "{stdout}"
    );
    // And no attempt to render the bytes.
    assert!(!stdout.contains("IHDR"), "{stdout}");
}

#[test]
fn several_changed_files_are_separated_in_the_output() {
    let repo = Repo::new("several");
    seed(&repo);
    repo.write("src/main.rs", "fn main() {\n    println!(\"two\");\n}\n");
    repo.write(
        "src/lib.rs",
        "pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n",
    );

    let (stdout, _) = repo.gdiff(&["git", "--view", "unified"]);
    assert_eq!(stdout.matches("--- a/").count(), 2, "{stdout}");
    assert_eq!(stdout.matches("+++ b/").count(), 2, "{stdout}");
}

#[test]
fn a_bad_revision_is_trouble_with_a_message() {
    let repo = Repo::new("badrev");
    seed(&repo);

    let output = Command::new(env!("CARGO_BIN_EXE_gdiff"))
        .current_dir(&repo.dir)
        .args(["git", "no-such-revision"])
        .output()
        .expect("the binary runs");

    assert_eq!(output.status.code(), Some(2), "exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-revision"), "{stderr}");
}

#[test]
fn outside_a_repository_it_says_so() {
    let dir = std::env::temp_dir().join(format!("gdiff-norepo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let output = Command::new(env!("CARGO_BIN_EXE_gdiff"))
        .current_dir(&dir)
        .arg("git")
        .output()
        .expect("the binary runs");

    let _ = std::fs::remove_dir_all(&dir);

    // A temp dir could sit inside a repository on some machines; only assert
    // the failure shape when it genuinely did not find one.
    if output.status.code() == Some(2) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("repository"), "{stderr}");
    }
}

/// The pager path: git's own output, re-rendered.
#[test]
fn a_patch_on_stdin_renders_with_the_right_line_numbers() {
    let repo = Repo::new("patch");
    repo.write(
        "src/main.rs",
        &(1..=30)
            .map(|n| format!("let line{n} = {n};\n"))
            .collect::<String>(),
    );
    repo.commit("initial");
    repo.write(
        "src/main.rs",
        &(1..=30)
            .map(|n| {
                if n == 20 {
                    format!("let line{n} = {n}00;\n")
                } else {
                    format!("let line{n} = {n};\n")
                }
            })
            .collect::<String>(),
    );

    let patch = repo.git(&["diff", "HEAD"]);
    assert!(patch.contains("@@"), "expected a real patch: {patch}");

    let mut child = Command::new(env!("CARGO_BIN_EXE_gdiff"))
        .current_dir(&repo.dir)
        .args(["--patch", "--view", "split", "--width", "160"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the binary runs");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(patch.as_bytes())
            .expect("write the patch");
    }
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(1), "exit code");
    // The line numbers come from the hunk header, not from the top of the
    // patch: this change is at line 20, and a patch source that renumbered
    // from 1 would be worse than useless.
    assert!(stdout.contains(" 20 "), "expected line 20 in:\n{stdout}");
    assert!(stdout.contains("line20"), "{stdout}");
    // And the lines the patch never carried are accounted for, not forgotten.
    assert!(stdout.contains("unchanged line"), "{stdout}");
}
