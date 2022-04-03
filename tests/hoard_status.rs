mod common;

use common::tester::Tester;
use hoard::command::Command;
use std::fs;

const DEFAULT_CONTENT: &str = "default text";
const CHANGED_CONTENT: &str = "changed text";
const OTHER_CONTENT: &str = "other text";

const HOARD_NO_CHANGES: &str = "no_changes";
const HOARD_LOCAL_CHANGES: &str = "local_changes";
const HOARD_REMOTE_CHANGES: &str = "remote_changes";
const HOARD_MIXED_CHANGES: &str = "mixed_changes";
const HOARD_UNEXPECTED_CHANGES: &str = "unexpected_changes";

const STATUS_TOML: &str = r#"
exclusivity = [
    ["unix", "windows"]
]

[envs]
[envs.windows]
    os = ["windows"]
[[envs.windows.env]]
    var = "HOARD_TMP"
[envs.unix]
    os = ["linux", "macos"]
[[envs.unix.env]]
    var = "HOME"

[hoards]
[hoards.no_changes]
    "unix"    = "${HOME}/unchanged.txt"
    "windows" = "${HOARD_TMP}/unchanged.txt"
[hoards.local_changes]
    "unix"    = "${HOME}/local.txt"
    "windows" = "${HOARD_TMP}/local.txt"
[hoards.remote_changes]
    "unix"    = "${HOME}/remote.txt"
    "windows" = "${HOARD_TMP}/remote.txt"
[hoards.mixed_changes]
    "unix"    = "${HOME}/mixed.txt"
    "windows" = "${HOARD_TMP}/mixed.txt"
[hoards.unexpected_changes]
    "unix"    = "${HOME}/unexpected.txt"
    "windows" = "${HOARD_TMP}/unexpected.txt"
"#;

fn setup_no_changes(tester: &Tester) {
    let path = tester.home_dir().join("unchanged.txt");
    fs::write(&path, DEFAULT_CONTENT).expect("writing to file should succeed");
    tester.use_local_uuid();
    tester.expect_forced_command(Command::Backup {
        hoards: vec![HOARD_NO_CHANGES.parse().unwrap()],
    });
}

fn setup_local_changes(tester: &Tester) {
    let path = tester.home_dir().join("local.txt");
    fs::write(&path, DEFAULT_CONTENT).expect("writing to file should succeed");
    tester.use_remote_uuid();
    tester.expect_forced_command(Command::Backup {
        hoards: vec![HOARD_LOCAL_CHANGES.parse().unwrap()],
    });
    tester.use_local_uuid();
    tester.expect_forced_command(Command::Restore {
        hoards: vec![HOARD_LOCAL_CHANGES.parse().unwrap()],
    });
    fs::write(&path, CHANGED_CONTENT).expect("writing to file should succeed");
}

fn setup_remote_changes(tester: &Tester) {
    let path = tester.home_dir().join("remote.txt");
    fs::write(&path, DEFAULT_CONTENT).expect("writing to file should succeed");
    tester.use_local_uuid();
    tester.expect_forced_command(Command::Backup {
        hoards: vec![HOARD_REMOTE_CHANGES.parse().unwrap()],
    });
    tester.use_remote_uuid();
    tester.expect_forced_command(Command::Restore {
        hoards: vec![HOARD_REMOTE_CHANGES.parse().unwrap()],
    });
    fs::write(&path, CHANGED_CONTENT).expect("writing to file should succeed");
    tester.expect_forced_command(Command::Backup {
        hoards: vec![HOARD_REMOTE_CHANGES.parse().unwrap()],
    });
    fs::write(&path, DEFAULT_CONTENT).expect("writing to file should succeed");
}

fn setup_mixed_changes(tester: &Tester) {
    let path = tester.home_dir().join("mixed.txt");
    tester.use_local_uuid();
    fs::write(&path, DEFAULT_CONTENT).expect("writing to file should succeed");
    tester.expect_forced_command(Command::Backup {
        hoards: vec![HOARD_MIXED_CHANGES.parse().unwrap()],
    });
    tester.use_remote_uuid();
    fs::write(&path, CHANGED_CONTENT).expect("writing to file should succeed");
    tester.expect_forced_command(Command::Backup {
        hoards: vec![HOARD_MIXED_CHANGES.parse().unwrap()],
    });
    tester.use_local_uuid();
    fs::write(&path, OTHER_CONTENT).expect("writing to file should succeed");
}

fn setup_unexpected_changes(tester: &Tester) {
    let path = tester.home_dir().join("unexpected.txt");
    let hoard_path = tester.data_dir().join("hoards").join("unexpected_changes");
    tester.use_local_uuid();
    fs::write(&path, DEFAULT_CONTENT).expect("writing to file should succeed");
    tester.expect_forced_command(Command::Backup {
        hoards: vec![HOARD_UNEXPECTED_CHANGES.parse().unwrap()],
    });
    fs::write(&hoard_path, CHANGED_CONTENT).expect("writing to file should succeed");
}

#[test]
fn test_hoard_status() {
    let tester = Tester::new(STATUS_TOML);
    setup_no_changes(&tester);
    setup_local_changes(&tester);
    setup_remote_changes(&tester);
    setup_mixed_changes(&tester);
    setup_unexpected_changes(&tester);

    tester.use_local_uuid();
    tester.expect_command(Command::Status);

    tester.assert_has_output("no_changes: up to date\n");
    tester.assert_has_output(
        "local_changes: modified locally -- sync with `hoard backup local_changes`\n",
    );
    tester.assert_has_output(
        "remote_changes: modified remotely -- sync with `hoard restore remote_changes`\n",
    );
    tester.assert_has_output("mixed_changes: mixed changes -- manual intervention recommended (see `hoard diff mixed_changes`)\n");
    tester.assert_has_output("unexpected_changes: unexpected changes -- manual intervention recommended (see `hoard diff unexpected_changes`)\n");
}
