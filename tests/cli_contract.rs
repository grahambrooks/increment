//! What the binary promises at its edges, exercised through the binary itself.
//!
//! These are the claims that a unit test cannot make: the process really does
//! exit with the documented code, and it really does refuse to write an
//! alternate screen into a pipe.

use std::process::Command;

fn gdiff() -> Command {
    Command::new(env!("CARGO_BIN_EXE_gdiff"))
}

#[test]
fn a_redirected_tui_request_exits_with_trouble_and_says_why() {
    // stdout is captured here, so it is not a terminal — the same shape as
    // `gdiff --ui tui a b > out.txt`.
    let output = gdiff()
        .args(["--ui", "tui", "a.rs", "b.rs"])
        .output()
        .expect("the binary runs");

    assert_eq!(output.status.code(), Some(2), "exit code");
    assert!(
        output.stdout.is_empty(),
        "nothing should reach a redirected stdout, got {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("needs a terminal"),
        "stderr should explain the refusal, got {stderr:?}"
    );
}

#[test]
fn version_reports_the_calver_version() {
    let output = gdiff().arg("--version").output().expect("the binary runs");

    assert!(output.status.success(), "--version should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "--version should print {}, got {stdout:?}",
        env!("CARGO_PKG_VERSION")
    );
}
