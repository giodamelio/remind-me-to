use std::fs;
use std::process::Command;

use insta_cmd::{assert_cmd_snapshot, get_cargo_bin};
use tempfile::TempDir;

/// Build a `std::process::Command` pointing at our binary with colors off
/// and tracing suppressed for deterministic snapshots.
fn cmd() -> Command {
    let mut c = Command::new(get_cargo_bin("remind-me-to"));
    c.arg("--color").arg("never");
    c.env("RUST_LOG", "off");
    c.env("NO_COLOR", "1");
    // Ensure no token leaks into tests.
    c.env_remove("GITHUB_TOKEN");
    c.env_remove("GH_TOKEN");
    c
}

/// Build an `assert_cmd::Command` (for exit-code assertions).
fn assert_cmd() -> assert_cmd::Command {
    let mut c = assert_cmd::Command::cargo_bin("remind-me-to").unwrap();
    c.arg("--color").arg("never");
    c.env("RUST_LOG", "off");
    c.env("NO_COLOR", "1");
    c.env_remove("GITHUB_TOKEN");
    c.env_remove("GH_TOKEN");
    c
}

// ---------------------------------------------------------------------------
// --help / --version
// ---------------------------------------------------------------------------

#[test]
fn help_output() {
    assert_cmd_snapshot!(cmd().arg("--help"));
}

#[test]
fn check_help_output() {
    assert_cmd_snapshot!(cmd().arg("check").arg("--help"));
}

#[test]
fn version_output() {
    assert_cmd_snapshot!(cmd().arg("--version"));
}

// ---------------------------------------------------------------------------
// dry-run: text format
// ---------------------------------------------------------------------------

#[test]
fn dry_run_single_reminder() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "// REMIND-ME-TO: Remove workaround pr_merged=github:tokio-rs/tokio#5432\n",
    )
    .unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd()
            .arg("check")
            .arg("--dry-run")
            .arg(dir.path()));
    });
}

#[test]
fn dry_run_multiple_reminders_one_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.rs");
    fs::write(
        &file,
        "\
// REMIND-ME-TO: Fix A pr_merged=github:owner/repo#1
fn middle() {}
// REMIND-ME-TO: Fix B tag_exists=github:owner/repo@>=2.0.0
",
    )
    .unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd()
            .arg("check")
            .arg("--dry-run")
            .arg(&file));
    });
}

#[test]
fn dry_run_no_reminders() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("clean.rs"), "fn main() {}\n").unwrap();

    assert_cmd_snapshot!(cmd()
        .arg("check")
        .arg("--dry-run")
        .arg(dir.path()));
}

#[test]
fn dry_run_parse_error() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("bad.rs"),
        "// REMIND-ME-TO: fix pr_merged=invalid_value\n",
    )
    .unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd()
            .arg("check")
            .arg("--dry-run")
            .arg(dir.path()));
    });
}

#[test]
fn dry_run_mixed_valid_and_error() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("mixed.rs"),
        "// REMIND-ME-TO: fix pr_merged=bad tag_exists=github:a/b@>=1.0\n",
    )
    .unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd()
            .arg("check")
            .arg("--dry-run")
            .arg(dir.path()));
    });
}

// ---------------------------------------------------------------------------
// dry-run: JSON format
// ---------------------------------------------------------------------------

#[test]
fn dry_run_json_format() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "// REMIND-ME-TO: Remove workaround pr_merged=github:tokio-rs/tokio#5432\n",
    )
    .unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd()
            .arg("check")
            .arg("--dry-run")
            .arg("--format")
            .arg("json")
            .arg(dir.path()));
    });
}

// ---------------------------------------------------------------------------
// quiet mode
// ---------------------------------------------------------------------------

#[test]
fn quiet_produces_no_output() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "// REMIND-ME-TO: fix pr_merged=github:a/b#1\n",
    )
    .unwrap();

    assert_cmd_snapshot!(cmd()
        .arg("check")
        .arg("--dry-run")
        .arg("--quiet")
        .arg(dir.path()));
}

// ---------------------------------------------------------------------------
// exit codes
// ---------------------------------------------------------------------------

#[test]
fn exit_code_0_no_reminders() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("clean.rs"), "fn main() {}\n").unwrap();

    assert_cmd()
        .arg("check")
        .arg("--dry-run")
        .arg(dir.path())
        .assert()
        .success();
}

#[test]
fn exit_code_2_parse_errors() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("bad.rs"),
        "// REMIND-ME-TO: fix pr_merged=invalid\n",
    )
    .unwrap();

    assert_cmd()
        .arg("check")
        .arg("--dry-run")
        .arg(dir.path())
        .assert()
        .code(2);
}

// ---------------------------------------------------------------------------
// comment styles
// ---------------------------------------------------------------------------

#[test]
fn dry_run_rust_comment() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(&file, "// REMIND-ME-TO: rust pr_merged=github:a/b#1\n").unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd().arg("check").arg("--dry-run").arg(&file));
    });
}

#[test]
fn dry_run_python_comment() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.py");
    fs::write(&file, "# remind-me-to: python tag_exists=github:c/d@>=1.0\n").unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd().arg("check").arg("--dry-run").arg(&file));
    });
}

#[test]
fn dry_run_lua_comment() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.lua");
    fs::write(&file, "-- Remind-Me-To: lua issue_closed=github:e/f#3\n").unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd().arg("check").arg("--dry-run").arg(&file));
    });
}

#[test]
fn dry_run_html_comment() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.html");
    fs::write(&file, "<!-- REMIND-ME-TO: html date_passed=2025-01-01 -->\n").unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd().arg("check").arg("--dry-run").arg(&file));
    });
}

// ---------------------------------------------------------------------------
// markers inside strings are ignored
// ---------------------------------------------------------------------------

#[test]
fn ignores_markers_in_string_literals() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("test.rs"),
        r#"
fn main() {
    let s = "// REMIND-ME-TO: not real pr_merged=github:a/b#1";
    println!("{}", s);
}
"#,
    )
    .unwrap();

    assert_cmd_snapshot!(cmd()
        .arg("check")
        .arg("--dry-run")
        .arg(dir.path()));
}

// ---------------------------------------------------------------------------
// description-only is not emitted
// ---------------------------------------------------------------------------

#[test]
fn description_only_not_emitted() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("note.rs"),
        "// REMIND-ME-TO: just a note with no operations\n",
    )
    .unwrap();

    assert_cmd_snapshot!(cmd()
        .arg("check")
        .arg("--dry-run")
        .arg(dir.path()));
}

// ---------------------------------------------------------------------------
// single file path
// ---------------------------------------------------------------------------

#[test]
fn dry_run_single_file_path() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("single.rs");
    fs::write(
        &file,
        "// REMIND-ME-TO: update pr_merged=github:owner/repo#42\n",
    )
    .unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd()
            .arg("check")
            .arg("--dry-run")
            .arg(&file));
    });
}

// ---------------------------------------------------------------------------
// multiple operations on one line
// ---------------------------------------------------------------------------

#[test]
fn dry_run_multiple_ops_one_line() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("multi.rs"),
        "// REMIND-ME-TO: Remove TLS hack pr_merged=github:hyper/hyper#100 tag_exists=github:hyper/hyper@>=2.0\n",
    )
    .unwrap();

    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(dir.path().to_str().unwrap()), "[DIR]");
    settings.bind(|| {
        assert_cmd_snapshot!(cmd()
            .arg("check")
            .arg("--dry-run")
            .arg(dir.path()));
    });
}
