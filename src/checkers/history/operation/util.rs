use std::fs;
use std::path::{Path, PathBuf};
use once_cell::sync::Lazy;
use regex::Regex;
use time::format_description::FormatItem;
use uuid::Uuid;
use crate::checkers::history;
use crate::checkers::history::operation::OperationImpl;
use super::{Operation, Error};

pub(crate) static TIME_FORMAT: Lazy<Vec<FormatItem<'static>>> = Lazy::new(|| {
    time::format_description::parse(
        "[year]_[month]_[day]-[hour repr:24]_[minute]_[second].[subsecond digits:6]",
    )
        .unwrap()
});

pub(crate) static LOG_FILE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^[0-9]{4}(_[0-9]{2}){2}-([0-9]{2}_){2}([0-9]{2})\\.[0-9]{6}\\.log$")
        .expect("invalid log file regex")
});

pub(crate) fn file_is_log(path: &Path) -> bool {
    let _span = tracing::trace_span!("file_is_log", ?path).entered();
    let result = path.is_file()
        && match path.file_name() {
        None => false, // grcov: ignore
        Some(name) => match name.to_str() {
            None => false, // grcov: ignore
            Some(name) => LOG_FILE_REGEX.is_match(name),
        },
    };
    tracing::trace!(result, "determined if file is operation log");
    result
}

/// Cleans up residual operation logs, leaving the latest per (system, hoard) pair.
///
/// Technically speaking, this function may leave up to two log files behind per pair.
/// If the most recent log file is for a *restore* operation, the most recent *backup* will
/// also be retained. If the most recent log file is a *backup*, it will be the only one
/// retained.
///
/// # Errors
///
/// - Any I/O error from working with and deleting multiple files
/// - Any [`Error`]s from parsing files to determine whether or not to keep them
pub(crate) fn cleanup_operations() -> Result<u32, (u32, Error)> {
    // Get hoard history root
    // Iterate over every uuid in the directory
    let root = history::get_history_root_dir();
    fs::read_dir(root)
        .map_err(|err| (0, err.into()))?
        .filter(|entry| {
            entry.as_ref().map_or_else(
                // Propagate errors
                |_err| true,
                // Keep only entries that are directories with UUIDs for names
                |entry| {
                    tracing::trace!("checking if {} is a system directory", entry.path().display());
                    entry.path().is_dir()
                        && entry
                        .file_name()
                        .to_str()
                        .map_or_else(|| false, |s| {
                            tracing::trace!("checking if {} is a valid UUID", s);
                            Uuid::parse_str(s).is_ok()
                        })
                },
            )
        })
        // For each system folder, make a list of all log files, excluding 1 or 2 to keep.
        .map(|entry| {
            let entry = entry?;
            let hoards = fs::read_dir(entry.path())?
                .map(|entry| entry.map(|entry| {
                    let path = entry.path();
                    tracing::trace!("found hoard directory: {}", path.display());
                    path
                }))
                .collect::<Result<Vec<_>, _>>()?;

            // List all log files in a hoard folder for the current iterated system
            hoards.into_iter()
                // Filter out last_paths.json
                .filter(|path| path.is_dir())
                .map(|path| {
                    tracing::trace!("checking files in directory: {}", path.display());
                    let mut files: Vec<PathBuf> = fs::read_dir(path)?
                        .filter_map(|subentry| {
                            subentry
                                .map(|subentry| {
                                    tracing::trace!("checking if {} is a log file", subentry.path().display());
                                    file_is_log(&subentry.path()).then(|| subentry.path())
                                })
                                .map_err(Error::from)
                                .transpose()
                        })
                        .collect::<Result<_, Error>>()?;

                    files.sort_unstable();

                    // The last item is the latest operation for this hoard, so keep it.
                    let recent = files.pop();

                    // Make sure the most recent backup is (also) retained.
                    if let Some(recent) = recent {
                        let recent = Operation::from_file(&recent)?;
                        if !recent.is_backup() {
                            tracing::trace!("most recent log is not a backup, making sure to retain a backup log too");
                            // Find the index of the latest backup
                            let index = files
                                .iter()
                                .enumerate()
                                .rev()
                                .find_map(|(i, path)| {
                                    Operation::from_file(path)
                                        .map(|op| op.is_backup().then(|| i))
                                        .transpose()
                                })
                                .transpose()?;

                            if let Some(index) = index {
                                // Found index of latest backup, remove it from deletion list
                                files.remove(index);
                            }
                        }
                    } // grcov: ignore

                    Ok(files)
                }).collect::<Result<Vec<_>, _>>()
        })
        // Collect a list of all files to delete for each system directory.
        .collect::<Result<Vec<Vec<Vec<PathBuf>>>, _>>()
        .map_err(|err| (0, err))?
        .into_iter()
        // Flatten the list of lists into a single list.
        .flatten()
        .flatten()
        // Delete each file.
        .map(|path| {
            tracing::trace!("deleting {}", path.display());
            fs::remove_file(path)
        })
        // Return the first error or the number of files deleted.
        .fold(Ok((0, ())), |acc, res2| {
            let (count, _) = acc?;
            Ok((count + 1, res2.map_err(|err| (count, err.into()))?))
        })
        .map(|(count, _)| count)
}
