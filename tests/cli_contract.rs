//! What the binary promises at its edges, exercised through the binary itself.
//!
//! These are the claims a unit test cannot make: the process really does exit
//! with the documented code, it really does refuse to write an alternate screen
//! into a pipe, and it really does strip colour when its output is not a
//! terminal.

use std::io::Write;
use std::process::Command;

fn increment() -> Command {
    Command::new(env!("CARGO_BIN_EXE_inc"))
}

/// Two temporary files with the given contents, in a directory of their own.
struct Pair {
    dir: std::path::PathBuf,
    old: std::path::PathBuf,
    new: std::path::PathBuf,
}

impl Pair {
    fn new(name: &str, old: &str, new: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("increment-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let write = |file: &str, text: &str| {
            let path = dir.join(file);
            let mut handle = std::fs::File::create(&path).expect("create");
            handle.write_all(text.as_bytes()).expect("write");
            path
        };
        Self {
            old: write("old.txt", old),
            new: write("new.txt", new),
            dir,
        }
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn identical_files_exit_zero_and_print_nothing() {
    let pair = Pair::new("same", "alpha\nbeta\n", "alpha\nbeta\n");
    let output = increment()
        .args([&pair.old, &pair.new])
        .output()
        .expect("the binary runs");

    assert_eq!(output.status.code(), Some(0), "exit code");
    assert!(
        output.stdout.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn differing_files_exit_one_like_diff_does() {
    let pair = Pair::new("differ", "let x = 1;\n", "let x = 2;\n");
    let output = increment()
        .args([&pair.old, &pair.new])
        .output()
        .expect("the binary runs");

    // 1 means "they differ", not "something went wrong" — the convention
    // anything already wrapping `diff` or `git diff --exit-code` expects.
    assert_eq!(output.status.code(), Some(1), "exit code");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("let x = 1;"), "{stdout}");
    assert!(stdout.contains("let x = 2;"), "{stdout}");
}

#[test]
fn output_to_a_pipe_carries_no_escape_codes() {
    // stdout is captured here, so it is not a terminal. Colour must strip
    // itself; a diff redirected into a file should be readable text.
    let pair = Pair::new("piped", "let x = 1;\n", "let x = 2;\n");
    let output = increment()
        .args([&pair.old, &pair.new])
        .output()
        .expect("the binary runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains('\u{1b}'),
        "escape codes in a pipe: {stdout:?}"
    );
}

#[test]
fn a_missing_file_is_trouble_not_a_difference() {
    let pair = Pair::new("missing", "a\n", "a\n");
    let output = increment()
        .args([
            pair.old.as_path(),
            std::path::Path::new("/nonexistent/increment"),
        ])
        .output()
        .expect("the binary runs");

    assert_eq!(output.status.code(), Some(2), "exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("/nonexistent/increment"), "{stderr}");
    // …and only that one. Naming the file that was fine sends the reader to
    // look at the wrong thing — `contains` alone let that through once.
    assert!(
        !stderr.contains(&pair.old.display().to_string()),
        "the error names the file that was readable too: {stderr}"
    );
}

#[test]
fn json_output_parses() {
    let pair = Pair::new("json", "let x = 1;\n", "let x = 2;\n");
    let output = increment()
        .args([pair.old.as_path(), pair.new.as_path()])
        .args(["--format", "json"])
        .output()
        .expect("the binary runs");

    assert_eq!(output.status.code(), Some(1), "exit code");
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
    assert_eq!(value["stats"]["modified"], 1);
    assert_eq!(value["rows"][0]["kind"], "modified");
}

#[test]
fn a_redirected_tui_request_exits_with_trouble_and_says_why() {
    let pair = Pair::new("tui", "a\n", "b\n");
    let output = increment()
        .args(["--ui", "tui"])
        .args([pair.old.as_path(), pair.new.as_path()])
        .output()
        .expect("the binary runs");

    assert_eq!(output.status.code(), Some(2), "exit code");
    assert!(
        output.stdout.is_empty(),
        "nothing should reach a redirected stdout"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("needs a terminal"), "{stderr}");
}

/// `inc a b | head` must not print an error after the reader goes away.
#[cfg(unix)]
#[test]
fn a_closed_pipe_is_a_normal_end_not_an_error() {
    let old: String = (1..=500).map(|n| format!("line {n}\n")).collect();
    let new: String = (1..=500)
        .map(|n| {
            if n % 7 == 0 {
                format!("LINE {n} changed\n")
            } else {
                format!("line {n}\n")
            }
        })
        .collect();
    let pair = Pair::new("pipe", &old, &new);

    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} --full '{}' '{}' | head -2",
            env!("CARGO_BIN_EXE_inc").replace('\'', "'\\''"),
            pair.old.display(),
            pair.new.display()
        ))
        .output()
        .expect("the shell runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "expected no diagnostics, got {stderr:?}");
}

#[test]
fn version_reports_the_calver_version() {
    let output = increment()
        .arg("--version")
        .output()
        .expect("the binary runs");

    assert!(output.status.success(), "--version should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "--version should print {}, got {stdout:?}",
        env!("CARGO_PKG_VERSION")
    );
}
