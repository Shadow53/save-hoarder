mod common;

use std::collections::BTreeMap;
use common::tester::Tester;
use hoard::command::Command;
use hoard::newtypes::HoardName;
use paste::paste;
use std::fs;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const DIFF_TOML: &str = r#"
exclusivity = [
    ["first", "second"],
    ["unix", "windows"]
]

[envs]
[envs.windows]
    os = ["windows"]
[[envs.windows.env]]
    var = "HOMEPATH"
[envs.unix]
    os = ["linux", "macos"]
[[envs.unix.env]]
    var = "HOME"


[hoards]
[hoards.anon_txt]
    "unix"    = "${HOME}/anon.txt"
    "windows" = "${HOARD_TMP}/anon.txt"

[hoards.anon_bin]
    "unix"    = "${HOME}/anon.bin"
    "windows" = "${HOARD_TMP}/anon.bin"

[hoards.named]
[hoards.named.text]
    "unix"    = "${HOME}/named.txt"
    "windows" = "${HOARD_TMP}/named.txt"
[hoards.named.binary]
    "unix"    = "${HOME}/named.bin"
    "windows" = "${HOARD_TMP}/named.bin"

[hoards.anon_dir]
    config = { ignore = ["*ignore*"] }
    "unix"    = "${HOME}/testdir"
    "windows" = "${HOARD_TMP}/testdir"
"#;

fn get_hoards(tester: &Tester) -> BTreeMap<HoardName, Vec<File>> {
    maplit::btreemap! {
        "anon_dir".parse().unwrap() => vec![
            File {
                path: tester.home_dir().join("testdir").join("test.txt"),
                hoard_path: Some(tester.data_dir().join("hoards").join("anon_dir").join("test.txt")),
                ignored: false,
                is_text: true,
            },
            File {
                path: tester.home_dir().join("testdir").join("test.bin"),
                hoard_path: Some(tester.data_dir().join("hoards").join("anon_dir").join("test.bin")),
                ignored: false,
                is_text: true,
            },
            File {
                path: tester.home_dir().join("testdir").join("ignore.txt"),
                hoard_path: None,
                is_text: true,
                ignored: true,
            },
        ],
        "anon_txt".parse().unwrap() => vec![
            File {
                path: tester.home_dir().join("anon.txt"),
                hoard_path: Some(tester.data_dir().join("hoards").join("anon_txt")),
                ignored: false,
                is_text: true,
            },
        ],
        "named".parse().unwrap() => vec![
            File {
                path: tester.home_dir().join("named.txt"),
                hoard_path: Some(tester.data_dir().join("hoards").join("named").join("text")),
                ignored: false,
                is_text: true,
            },
            File {
                path: tester.home_dir().join("named.bin"),
                hoard_path: Some(tester.data_dir().join("hoards").join("named").join("binary")),
                ignored: false,
                is_text: true,
            },
        ],
    }
}

fn modify_file(path: &Path, content: Option<Content>, is_text: bool) {
    match content {
        None => {
            if path.exists() {
                fs::remove_file(path).expect("removing file should succeed");
                assert!(!path.exists());
            }
        }
        Some(Content::Data((text, binary))) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("should be able to create file parents");
            }

            if is_text {
                fs::write(path, text).expect("writing text to file should succeed");
            } else {
                fs::write(path, binary).expect("writing text to file should succeed");
            }

            assert!(
                path.exists(),
                "writing to the {} failed to create file",
                path.display()
            );
        }
        Some(Content::Perms(octet)) => {
            let file = fs::File::open(path).expect("file should exist and be able to be opened");
            let mut permissions = file
                .metadata()
                .expect("failed to read file metadata")
                .permissions();
            #[cfg(unix)]
            {
                permissions.set_mode(octet);
                fs::set_permissions(path, permissions).expect("failed to set permissions on file");
            }
            #[cfg(windows)]
            {
                let readonly = !is_writable(octet);
                if permissions.readonly() != readonly {
                    println!(
                        "attempting to set {} permissions to {} from readonly = {}",
                        path.display(),
                        !is_writable(octet),
                        permissions.readonly()
                    );
                    permissions.set_readonly(readonly);
                    fs::set_permissions(path, permissions)
                        .expect("failed to set permissions on file");
                }
            }
        }
    }
}

fn assert_content(path: &Path, content: Option<Content>, is_text: bool) {
    let file_content = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(err) => match err.kind() {
            ErrorKind::NotFound => None,
            _ => panic!("failed to read contents of {}: {}", path.display(), err),
        }
    };

    match (content, file_content) {
        (None, None) => {},
        (None, Some(_)) => {
            panic!("expected {} to not exist, but it does", path.display());
        }
        (Some(_), None) => {
            panic!("expected {} to exist, but it does not", path.display());
        }
        (Some(Content::Data((text, binary))), Some(current_data)) => {
            if is_text {
                let current_text = String::from_utf8(current_data).unwrap();
                assert_eq!(current_text, text, "expected file to contain right value, but had left value instead");
            } else {
                assert_eq!(current_data, binary, "expected file to contain right value, but had left value instead");
            }
        }
        (Some(Content::Perms(perms)), Some(_)) => {
            unimplemented!("permissions checking is not implemented yet");
        }
    }
}

fn assert_diff_contains(
    tester: &Tester,
    hoard: &HoardName,
    content: String,
    is_partial: bool,
    invert: bool,
    is_verbose: bool,
) {
    tester.use_local_uuid();
    tester.expect_command(Command::Diff {
        hoard: hoard.clone(),
        verbose: is_verbose,
    });
    if invert {
        tester.assert_not_has_output(&content);
    } else if is_partial {
        tester.assert_has_output(&content);
    } else {
        let debug_output = ""; //tester.extra_logging_output();
        assert_eq!(tester.output(), content, "{}", debug_output);
    }
}

fn get_full_diff(
    file: &File,
    hoard_content: Option<Content>,
    system_content: Option<Content>,
) -> String {
    let hoard_content = match hoard_content {
        None => return String::new(),
        Some(Content::Data((hoard_content, _))) => hoard_content,
        Some(_) => panic!("expected text, not permissions"),
    };

    let system_content = match system_content {
        None => return String::new(),
        Some(Content::Data((system_content, _))) => system_content,
        Some(_) => panic!("expected text, not permissions"),
    };

    if file.is_text && file.hoard_path.is_some() && hoard_content != system_content {
        format!(
            r#"--- {}
+++ {}
@@ -1 +1 @@
-{}
\ No newline at end of file
+{}
\ No newline at end of file

"#,
            file.hoard_path.as_ref().unwrap().display(),
            file.path.display(),
            hoard_content,
            system_content
        )
    } else {
        String::new()
    }
}

struct File {
    path: PathBuf,
    hoard_path: Option<PathBuf>,
    is_text: bool,
    ignored: bool,
}

#[derive(Clone)]
enum Content {
    Data((&'static str, [u8; 5])),
    Perms(u32),
}

impl Content {
    fn default() -> Option<Self> {
        Some(Content::Data(("This is a text file", [0x12, 0xFB, 0x3D, 0x00, 0x3A])))
    }

    fn changed_a() -> Option<Self> {
        Some(Content::Data((
            "This is different text content",
            [0x12, 0xFB, 0x45, 0x00, 0x3A],
        )))
    }

    fn changed_b() -> Option<Self> {
       Some(Content::Data((
           "This is yet other text content",
           [0x12, 0xFB, 0x91, 0x00, 0x3A],
       )))
    }

    fn none() -> Option<Self> {
        None
    }
}

// SITUATIONS LEFT TO HANDLE:
// Unexpected -- File created locally and in hoard with different text
// Unexpected -- Same modification to binary in system and hoard
// Unexpected -- Different modification to binary in system and hoard
// Unexpected -- Local deleted, binary modified in hoard
// Unexpected -- No records, create in hoard
// Unexpected -- No local changes, log created remotely, deleted in hoard
// Unexpected -- No local changes, log modified remotely, deleted in hoard
// Expected -- Log created and deleted, created locally
// Unexpected -- Locally modified, deleted in hoard
// Unexpected -- Log created remotely, deleted in hoard
// Unexpected -- Log modified remotely, deleted in hoard
// Expected -- Log deleted locally, Log created and deleted remotely, created locally
// Unexpected -- Log deleted locally, log created and deleted remotely, created in hoard
// Unexpected -- Log deleted Remotely, delete locally and create in hoard
// Expected -- Delete and recreate binary remotely
// Expected -- Modify binary remotely
// Unexpected -- Log deleted remotely, recreate in hoard with different binary content
// Unexpected -- Log deleted, create locally and in hoard with different binary content
// Expected -- Log delete and create remotely, modify locally (binary)
// Unexpected -- Exists locally, Log deleted remotely, recreate with different binary content in hoard
// Unexpected -- No records, create in hoard (ln 610)
// Unexpected -- Log create and delete, create locally, create in hoard with different text content
// Unexpectd -- Log delete remotely, recreate in hoard with different text content
// Expected -- Create locally, log create and modify remotely, different text content
// Unexpected -- No records, Create locally and in hoard with different text content

// Permissions?

macro_rules! test_diff_inner {
    (
        tester: $tester:ident,
        hoard_name: $hoard_name:ident,
        hoard_content: $hoard_content:ident,
        system_content: $system_content:ident,
        other_content: $other_content:ident,
        file: $file:ident,
        setup: {}
    ) => {};
    (
        tester: $tester:ident,
        hoard_name: $hoard_name:ident,
        hoard_content: $hoard_content:ident,
        system_content: $system_content:ident,
        other_content: $other_content:ident,
        file: $file:ident,
        setup: {backup; $($ops:tt)*}
    ) => {
        $hoard_content = $system_content.clone();
        $tester.expect_command(Command::Backup { hoards: vec![$hoard_name.clone()] });
        if let Some(hoard_path) = $file.hoard_path.as_deref() {
            assert_content(hoard_path, $hoard_content.clone(), $file.is_text);
        }
        test_diff_inner! { tester: $tester, hoard_name: $hoard_name, hoard_content: $hoard_content, system_content: $system_content, other_content: $other_content, file: $file, setup: {$($ops)*} }
    };
    (
        tester: $tester:ident,
        hoard_name: $hoard_name:ident,
        hoard_content: $hoard_content:ident,
        system_content: $system_content:ident,
        other_content: $other_content:ident,
        file: $file:ident,
        setup: {restore; $($ops:tt)*}
    ) => {
        $system_content = $hoard_content.clone();
        $tester.expect_command(Command::Restore { hoards: vec![$hoard_name.clone()] });
        if $file.hoard_path.is_some() {
            assert_content(&$file.path, $system_content.clone(), $file.is_text);
        }
        test_diff_inner! { tester: $tester, hoard_name: $hoard_name, hoard_content: $hoard_content, system_content: $system_content, other_content: $other_content, file: $file, setup: {$($ops)*} }
    };
    (
        tester: $tester:ident,
        hoard_name: $hoard_name:ident,
        hoard_content: $hoard_content:ident,
        system_content: $system_content:ident,
        other_content: $other_content:ident,
        file: $file:ident,
        setup: {local; $($ops:tt)*}
    ) => {
        if $tester.current_uuid().as_ref() == Some($tester.remote_uuid()) {
            ::std::mem::swap(&mut $system_content, &mut $other_content);
            modify_file(&$file.path, $system_content.clone(), $file.is_text);
        }
        $tester.use_local_uuid();
        test_diff_inner! { tester: $tester, hoard_name: $hoard_name, hoard_content: $hoard_content, system_content: $system_content, other_content: $other_content, file: $file, setup: {$($ops)*} }
    };
    (
        tester: $tester:ident,
        hoard_name: $hoard_name:ident,
        hoard_content: $hoard_content:ident,
        system_content: $system_content:ident,
        other_content: $other_content:ident,
        file: $file:ident,
        setup: {remote; $($ops:tt)*}
    ) => {
        if $tester.current_uuid().as_ref() == Some($tester.local_uuid()) {
            ::std::mem::swap(&mut $system_content, &mut $other_content);
            modify_file(&$file.path, $system_content.clone(), $file.is_text);
        }
        $tester.use_remote_uuid();
        test_diff_inner! { tester: $tester, hoard_name: $hoard_name, hoard_content: $hoard_content, system_content: $system_content, other_content: $other_content, file: $file, setup: {$($ops)*} }
    };
    (
        tester: $tester:ident,
        hoard_name: $hoard_name:ident,
        hoard_content: $hoard_content:ident,
        system_content: $system_content:ident,
        other_content: $other_content:ident,
        file: $file:ident,
        setup: {set_system_content: $content:expr; $($ops:tt)*}
    ) => {
        $system_content = $content;
        modify_file(&$file.path, $content, $file.is_text);
        test_diff_inner! { tester: $tester, hoard_name: $hoard_name, hoard_content: $hoard_content, system_content: $system_content, other_content: $other_content, file: $file, setup: {$($ops)*} }
    };
    (
        tester: $tester:ident,
        hoard_name: $hoard_name:ident,
        hoard_content: $hoard_content:ident,
        system_content: $system_content:ident,
        other_content: $other_content:ident,
        file: $file:ident,
        setup: {set_hoard_content: $content:expr; $($ops:tt)*}
    ) => {
        if let Some(hoard_path) = $file.hoard_path.as_deref() {
            $hoard_content = $content;
            modify_file(hoard_path, $content, $file.is_text);
            test_diff_inner! { tester: $tester, hoard_name: $hoard_name, hoard_content: $hoard_content, system_content: $system_content, other_content: $other_content, file: $file, setup: {$($ops)*} }
        }
    };
}

macro_rules! test_diff {
    (
        name: $fn_name: ident,
        diff_type: $diff_type:ident,
        location: $location:ident,
        setup: {$($ops:tt)*}
    ) => {
        #[test]
        fn $fn_name() {
            let tester = Tester::with_log_level(DIFF_TOML, tracing::Level::INFO);
            let hoards = get_hoards(&tester);

            for (hoard_name, files) in hoards {
                for file in files {
                    let mut system_content = None;
                    let mut hoard_content = None;
                    let mut other_system_content = None;

                    test_diff_inner! {
                        tester: tester,
                        hoard_name: hoard_name,
                        hoard_content: hoard_content,
                        system_content: system_content,
                        other_content: other_system_content,
                        file: file,
                        setup: {$($ops)* local; }
                    }

                    let diff_str = match $diff_type {
                        CREATED | DELETED => format!("{} {}", $diff_type, $location),
                        MODIFIED => format!(
                            "{} file {} {}",
                            if file.is_text { "text" } else { "binary" },
                            $diff_type,
                            $location
                        ),
                        PERMS => {
                            let hoard_perms = match hoard_content.clone().expect("expected permissions") {
                                Content::Data(_) => panic!("expected permissions, not data"),
                                Content::Perms(perms) => perms,
                            };

                            let system_perms = match system_content.clone().expect("expected permissions") {
                                Content::Data(_) => panic!("expected permissions, not data"),
                                Content::Perms(perms) => perms,
                            };

                            #[cfg(unix)]
                            let (hoard_perms, system_perms) = (format!("{:o}", hoard_perms), format!("{:o}", system_perms));

                            #[cfg(windows)]
                            let hoard_perms = if is_writable(hoard_perms) {
                                "writable"
                            } else {
                                "readonly"
                            };

                            #[cfg(windows)]
                            let system_perms = if is_writable(system_perms) {
                                "writable"
                            } else {
                                "readonly"
                            };
                            format!("permissions changed: hoard({}), system ({})", hoard_perms, system_perms)
                        },
                        _ => panic!("unexpected diff type: {}", $diff_type),
                    };

                    let expected = format!(
                        "{}: {}\n",
                        file.path.display(),
                        diff_str
                    );

                    let expected_verbose = if file.is_text && system_content.is_some() && hoard_content.is_some() {
                        format!(
                            "{}{}",
                            expected,
                            get_full_diff(&file, hoard_content, system_content),
                        )
                    } else {
                        expected.clone()
                    };

                    assert_diff_contains(
                        &tester,
                        &hoard_name,
                        expected,
                        true,
                        file.ignored,
                        false,
                    );

                    assert_diff_contains(
                        &tester,
                        &hoard_name,
                        expected_verbose,
                        true,
                        file.ignored,
                        true,
                    );

                    tester.clear_data_dir();
                    if file.path.exists() {
                        fs::remove_file(&file.path).unwrap();
                    }
                }
            }
        }
    }
}

const CREATED: &str = "(re)created";
const MODIFIED: &str = "changed";
const PERMS: &str = "permissions changed";
const DELETED: &str = "deleted";

const LOCAL: &str = "locally";
const REMOTE: &str = "remotely";
const MIXED: &str = "locally and remotely";
const UNKNOWN: &str = "out-of-band";

mod create {
    use super::*;

    test_diff! {
        name: test_local,
        diff_type: CREATED,
        location: LOCAL,
        setup:  {
            local;
            set_system_content: Content::default();
        }
    }

    test_diff! {
        name: test_remote,
        diff_type: CREATED,
        location: REMOTE,
        setup: {
            remote;
            set_system_content: Content::default();
            backup;
        }
    }

    test_diff! {
        name: test_mixed_same_content,
        diff_type: CREATED,
        location: MIXED,
        setup: {
            remote;
            set_system_content: Content::default();
            backup;
            local;
            set_system_content: Content::default();
        }
    }

    test_diff! {
        name: test_mixed_different_content,
        diff_type: CREATED,
        location: MIXED,
        setup: {
            remote;
            set_system_content: Content::default();
            backup;
            local;
            set_system_content: Content::changed_a();
        }
    }

    test_diff! {
        name: test_out_of_band_only,
        diff_type: CREATED,
        location: UNKNOWN,
        setup: {
            set_hoard_content: Content::default();
        }
    }

    test_diff! {
        name: test_out_of_band_and_local_same_content,
        diff_type: CREATED,
        location: UNKNOWN,
        setup: {
            set_hoard_content: Content::default();
            set_system_content: Content::default();
        }
    }

    test_diff! {
        name: test_out_of_band_and_local_different_content,
        diff_type: CREATED,
        location: UNKNOWN,
        setup: {
            set_hoard_content: Content::default();
            set_system_content: Content::changed_a();
        }
    }
}

mod recreate {
    use super::*;

    mod local {
        use super::*;

        test_diff! {
            name: test_recreate_local_only,
            diff_type: CREATED,
            location: LOCAL,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                backup;
                set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: test_remote_create_and_delete_and_recreate_local,
            diff_type: CREATED,
            location: LOCAL,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_system_content: Content::none();
                backup;
                local;
                set_system_content: Content::changed_a();
            }
        }
    }

    mod remote {
        use super::*;

        test_diff! {
            name: test_create_delete_local_and_recreate_remote,
            diff_type: CREATED,
            location: REMOTE,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                backup;
                remote;
                restore;
                set_system_content: Content::default();
                backup;
            }
        }

        test_diff! {
            name: test_create_local_and_delete_recreate_remote,
            diff_type: CREATED,
            location: REMOTE,
            setup: {
                local; set_system_content: Content::default(); backup;
                remote; restore; set_system_content: Content::none(); backup;
                local; restore;
                remote; set_system_content: Content::changed_a(); backup;
            }
        }

        test_diff! {
            name: all_remote_with_local_restores,
            diff_type: CREATED,
            location: REMOTE,
            setup: {
                remote; set_system_content: Content::default(); backup;
                local; restore;
                remote; set_system_content: Content::none(); backup;
                local; restore;
                remote; set_system_content: Content::default(); backup;
            }
        }
    }

    mod mixed {
        use super::*;

        test_diff! {
            name: test_create_delete_local_recreate_both_same_content,
            diff_type: CREATED,
            location: MIXED,
            setup: {
                local; set_system_content: Content::default(); backup;
                remote; restore;
                local; set_system_content: Content::none(); backup;
                remote; restore; set_system_content: Content::default(); backup;
                local; set_system_content: Content::default();
            }
        }

        test_diff! {
            name: test_create_delete_local_recreate_both_different_content,
            diff_type: CREATED,
            location: MIXED,
            setup: {
                local; set_system_content: Content::default(); backup;
                remote; restore;
                local; set_system_content: Content::none(); backup;
                remote; restore; set_system_content: Content::default(); backup;
                local; set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: test_create_delete_local_recreate_both_same_content_no_restore,
            diff_type: CREATED,
            location: MIXED,
            setup: {
                local;
                set_system_content: Content::default(); backup;
                set_system_content: Content::none(); backup;
                remote; restore; set_system_content: Content::default(); backup;
                local; set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: test_create_delete_local_recreate_both_different_content_no_restore,
            diff_type: CREATED,
            location: MIXED,
            setup: {
                local;
                set_system_content: Content::default(); backup;
                set_system_content: Content::none(); backup;
                remote; restore; set_system_content: Content::default(); backup;
                local; set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: test_create_delete_remote_recreate_both_same_content,
            diff_type: CREATED,
            location: MIXED,
            setup: {
                remote;
                set_system_content: Content::default(); backup;
                set_system_content: Content::none(); backup;
                local; restore;
                remote; set_system_content: Content::changed_a(); backup;
                local; set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: test_create_delete_remote_recreate_both_different_content,
            diff_type: CREATED,
            location: MIXED,
            setup: {
                remote;
                set_system_content: Content::default(); backup;
                set_system_content: Content::none(); backup;
                local; restore;
                remote; set_system_content: Content::changed_a(); backup;
                local; set_system_content: Content::changed_b();
            }
        }
    }

    mod unexpected {
        use super::*;

        test_diff! {
            name: create_delete_locally,
            diff_type: CREATED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                backup;
                set_hoard_content: Content::default();
            }
        }

        test_diff! {
            name: create_delete_remotely,
            diff_type: CREATED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                backup;
                set_hoard_content: Content::default();
            }
        }

        test_diff! {
            name: create_delete_locally_create_local_same_content,
            diff_type: CREATED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                backup;
                set_hoard_content: Content::default();
                local;
                set_system_content: Content::default();
            }
        }

        test_diff! {
            name: create_delete_remotely_create_local_same_content,
            diff_type: CREATED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                backup;
                set_hoard_content: Content::default();
                local;
                set_system_content: Content::default();
            }
        }

        test_diff! {
            name: create_delete_locally_create_local_different_content,
            diff_type: CREATED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                backup;
                set_hoard_content: Content::changed_a();
                local;
                set_system_content: Content::changed_b();
            }
        }

        test_diff! {
            name: create_delete_remotely_create_local_different_content,
            diff_type: CREATED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                backup;
                set_hoard_content: Content::changed_a();
                local;
                set_system_content: Content::changed_b();
            }
        }
    }
}

mod modify {
    use super::*;

    mod local {
        use super::*;
        test_diff! {
            name: test_modify_local_only,
            diff_type: MODIFIED,
            location: LOCAL,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: test_modify_locally_from_remote_create,
            diff_type: MODIFIED,
            location: LOCAL,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_system_content: Content::changed_a();
            }
        }
    }

    mod remote {
        use super::*;

        test_diff! {
            name: test_create_local_modify_remote,
            diff_type: MODIFIED,
            location: REMOTE,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::changed_a();
                backup;
            }
        }

        test_diff! {
            name: test_create_modify_remote,
            diff_type: MODIFIED,
            location: REMOTE,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::changed_a();
                backup;
            }
        }

        test_diff! {
            name: create_local_delete_recreate_remote,
            diff_type: MODIFIED,
            location: REMOTE,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::none();
                backup;
                set_system_content: Content::changed_a();
                backup;
            }
        }

        test_diff! {
            name: create_remote_restore_local_delete_recreate_remote,
            diff_type: MODIFIED,
            location: REMOTE,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::none();
                backup;
                set_system_content: Content::changed_a();
                backup;
            }
        }
    }

    mod mixed {
        use super::*;

        test_diff! {
            name: create_local_modify_same_content_both,
            diff_type: MODIFIED,
            location: MIXED,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::changed_a();
                backup;
                local;
                set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: create_local_modify_different_content_both,
            diff_type: MODIFIED,
            location: MIXED,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::changed_a();
                backup;
                local;
                set_system_content: Content::changed_b();
            }
        }

        test_diff! {
            name: create_remote_modify_same_content_both,
            diff_type: MODIFIED,
            location: MIXED,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::changed_b();
                backup;
                local;
                set_system_content: Content::changed_b();
            }
        }

        test_diff! {
            name: create_remote_modify_different_content_both,
            diff_type: MODIFIED,
            location: MIXED,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::changed_b();
                backup;
                local;
                set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: create_local_modify_same_content_remote_delete_recreate,
            diff_type: MODIFIED,
            location: MIXED,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::none();
                backup;
                set_system_content: Content::changed_a();
                backup;
                local;
                set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: create_local_modify_different_content_remote_delete_recreate,
            diff_type: MODIFIED,
            location: MIXED,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::none();
                backup;
                set_system_content: Content::changed_a();
                backup;
                local;
                set_system_content: Content::changed_b();
            }
        }

        test_diff! {
            name: create_remote_modify_same_content_remote_delete_recreate,
            diff_type: MODIFIED,
            location: MIXED,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::none();
                backup;
                set_system_content: Content::changed_b();
                backup;
                local;
                set_system_content: Content::changed_b();
            }
        }

        test_diff! {
            name: create_remote_modify_different_content_remote_delete_recreate,
            diff_type: MODIFIED,
            location: MIXED,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::none();
                backup;
                set_system_content: Content::changed_b();
                backup;
                local;
                set_system_content: Content::changed_a();
            }
        }
    }

    mod unknown {
        use super::*;

        test_diff! {
            name: no_local_changes_last_edit_local,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: no_local_changes_last_edit_remote,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: no_local_logs_last_edit_remote,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: local_create_local_and_out_of_band_edit_same_content,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::changed_a();
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: local_create_local_and_out_of_band_edit_different_content,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::changed_a();
                set_hoard_content: Content::changed_b();
            }
        }

        test_diff! {
            name: local_create_local_delete_and_out_of_band_edit,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: remote_create_local_and_out_of_band_edit_same_content,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_system_content: Content::changed_a();
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: remote_create_local_and_out_of_band_edit_different_content,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_system_content: Content::changed_b();
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: remote_create_local_create_and_out_of_band_edit_same_content,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                set_system_content: Content::changed_a();
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: remote_create_local_create_and_out_of_band_edit_different_content,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                set_system_content: Content::changed_b();
                set_hoard_content: Content::changed_a();
            }
        }

        test_diff! {
            name: remote_create_local_delete_and_out_of_band_edit,
            diff_type: MODIFIED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_system_content: Content::none();
                set_hoard_content: Content::changed_a();
            }
        }
    }
}

mod permissions {
    use super::*;

}

mod delete {
    use super::*;

    mod local {
        use super::*;

        test_diff! {
            name: create_delete_local,
            diff_type: DELETED,
            location: LOCAL,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
            }
        }

        test_diff! {
            name: create_remote_delete_local,
            diff_type: DELETED,
            location: LOCAL,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_system_content: Content::none();
            }
        }

        test_diff! {
            name: create_local_modify_remote_delete_local,
            diff_type: DELETED,
            location: LOCAL,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::changed_a();
                backup;
                local;
                set_system_content: Content::none();
            }
        }

        test_diff! {
            name: create_remote_modify_remote_delete_local,
            diff_type: DELETED,
            location: LOCAL,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::changed_a();
                backup;
                local;
                set_system_content: Content::none();
            }
        }
    }

    mod remote {
        use super::*;

        test_diff! {
            name: create_local_delete_remote,
            diff_type: DELETED,
            location: REMOTE,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::none();
                backup;
                local;
                set_system_content: Content::default();
            }
        }

        test_diff! {
            name: create_modify_local_delete_remote,
            diff_type: DELETED,
            location: REMOTE,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::none();
                backup;
                local;
                set_system_content: Content::changed_a();
            }
        }

        test_diff! {
            name: create_remote_restore_modify_local_delete_remote,
            diff_type: DELETED,
            location: REMOTE,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::none();
                backup;
                local;
                set_system_content: Content::changed_a();
            }
        }
    }

    mod mixed {
        use super::*;

        test_diff! {
            name: create_local_delete_both,
            diff_type: DELETED,
            location: MIXED,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::none();
                backup;
                local;
                set_system_content: Content::none();
            }
        }

        test_diff! {
            name: create_remote_delete_both,
            diff_type: DELETED,
            location: MIXED,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                remote;
                set_system_content: Content::none();
                backup;
                local;
                set_system_content: Content::none();
            }
        }
    }

    mod unknown {
        use super::*;

        test_diff! {
            name: create_local_delete_from_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_hoard_content: Content::none();
            }
        }

        test_diff! {
            name: create_local_restore_remote_delete_from_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_hoard_content: Content::none();
            }
        }

        test_diff! {
            name: create_local_modify_remote_delete_from_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                remote;
                restore;
                set_system_content: Content::changed_a();
                set_hoard_content: Content::none();
            }
        }

        test_diff! {
            name: create_local_modify_local_delete_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::changed_a();
                set_hoard_content: Content::none();
            }
        }

        test_diff! {
            name: create_local_delete_local_and_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                local;
                set_system_content: Content::default();
                backup;
                set_system_content: Content::none();
                set_hoard_content: Content::none();
            }
        }

        test_diff! {
            name: create_remote_delete_from_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                set_hoard_content: Content::none();
            }
        }

        test_diff! {
            name: create_remote_restore_local_delete_from_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_hoard_content: Content::none();
            }
        }

        test_diff! {
            name: create_remote_modify_local_delete_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_system_content: Content::changed_a();
                set_hoard_content: Content::none();
            }
        }

        test_diff! {
            name: create_remote_delete_local_and_unknown,
            diff_type: DELETED,
            location: UNKNOWN,
            setup: {
                remote;
                set_system_content: Content::default();
                backup;
                local;
                restore;
                set_system_content: Content::none();
                set_hoard_content: Content::none();
            }
        }
    }
}