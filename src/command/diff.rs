use crate::hoard::iter::{HoardDiffIter, HoardFileDiff};
use crate::hoard::Hoard;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub(crate) fn run_diff(
    hoard: &Hoard,
    hoard_name: &str,
    hoards_root: &Path,
    verbose: bool,
) -> Result<(), super::Error> {
    let _span = tracing::trace_span!("run_diff").entered();
    tracing::trace!("running the diff command");
    let diff_iterator = HoardDiffIter::new(hoards_root, hoard_name.to_string(), hoard).map_err(super::Error::Diff)?;
    for hoard_diff in diff_iterator {
        tracing::trace!("printing diff: {:?}", hoard_diff);
        match hoard_diff.map_err(super::Error::Diff)? {
            HoardFileDiff::BinaryModified { file, diff_source } => {
                tracing::info!("{}: binary file changed {}", file.system_path().display(), diff_source);
            }
            HoardFileDiff::TextModified {
                file,
                unified_diff,
                diff_source,
            } => {
                tracing::info!("{}: text file changed {}", file.system_path().display(), diff_source);
                if verbose {
                    tracing::info!("{}", unified_diff);
                }
            }
            HoardFileDiff::PermissionsModified {
                file,
                hoard_perms,
                system_perms,
                ..
            } => {
                #[cfg(unix)]
                tracing::info!(
                    "{}: permissions changed: hoard ({:o}), system ({:o})",
                    file.system_path().display(),
                    hoard_perms.mode(),
                    system_perms.mode(),
                );
                #[cfg(not(unix))]
                tracing::info!(
                    "{}: permissions changed: hoard ({}), system ({})",
                    file.system_path.display(),
                    if hoard_perms.readonly() {
                        "readonly"
                    } else {
                        "writable"
                    },
                    if system_perms.readonly() {
                        "readonly"
                    } else {
                        "writable"
                    },
                );
            }
            HoardFileDiff::Created { file, diff_source } => {
                tracing::info!("{}: created {}", file.system_path().display(), diff_source);
            }
            HoardFileDiff::Recreated { file, diff_source } => {
                tracing::info!("{}: recreated {}", file.system_path().display(), diff_source);
            }
            HoardFileDiff::Deleted { file, diff_source } => {
                tracing::info!("{}: deleted {}", file.system_path().display(), diff_source);
            }
        }
    }

    Ok(())
}