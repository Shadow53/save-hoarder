//! Keeping track of all operations.
//!
//! The types in this module are used for logging all operations to disk. This information can be
//! used for debugging purposes, but is more directly used as a [`Checker`] to help prevent
//! synchronized changes from being overwritten.
//!
//! It does this by parsing synchronized logs from this and other systems to determine which system
//! was the last one to touch a file.

use crate::checkers::history::operation::{OperationFileInfo, OperationImpl};
use crate::hoard::iter::{OperationIter, OperationType};
use crate::hoard::{Direction, Hoard as ConfigHoard};
use crate::hoard_file::{Checksum, ChecksumType, HoardFile};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

use super::Error;

/// Errors that may occur while working with operation logs.

/// A single operation log.
///
/// This keeps track of the timestamp of the operation (which may include multiple hoards),
/// all hoards involved in the operation (and the related [`HoardOperation`]), and a record
/// of the latest operation log for each external system at the time of invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::module_name_repetitions)]
pub(crate) struct OperationV2 {
    /// Timestamp of last operation
    timestamp: OffsetDateTime,
    /// Which direction this operation went
    direction: Direction,
    /// The name of the hoard for this `HoardOperation`.
    hoard: String,
    /// Mapping of pile files to checksums
    files: Hoard,
    // TODO: include unchanged as well
    #[serde(skip, default)]
    hoards_root: PathBuf,
}

impl OperationV2 {
    pub(super) fn new(
        hoards_root: &Path,
        name: &str,
        hoard: &ConfigHoard,
        direction: Direction,
    ) -> Result<Self, Error> {
        Ok(Self {
            timestamp: OffsetDateTime::now_utc(),
            direction,
            hoard: name.into(),
            files: Hoard::new(hoards_root, name, hoard, direction)?,
            hoards_root: hoards_root.to_path_buf(),
        })
    }

    /// Convert a v1 Operation to a v2 Operation.
    ///
    /// Requires all of the operations prior to `old_v1` to have been processed such that the
    /// following is true:
    ///
    /// - `file_checksums`: contains all paths that were ever part of the hoard with their recorded
    ///   checksums. The checksum is `None` if the file did not exist prior to the `old_v1` operation.
    /// - `files`: contains all paths whose checksums are `None` in `file_checksums`. This is used
    ///   as an optimization technique while determining which files were created or deleted.
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn from_v1(
        file_checksums: &mut HashMap<(Option<String>, PathBuf), Option<Checksum>>,
        file_set: &mut HashSet<(Option<String>, PathBuf)>,
        old_v1: super::v1::OperationV1,
    ) -> Self {
        let mut these_files = HashSet::new();
        let mut files = HashMap::new();
        let is_anonymous = matches!(old_v1.hoard, super::v1::Hoard::Anonymous(_));

        for file_info in old_v1.all_files_with_checksums() {
            let OperationFileInfo {
                pile_name,
                relative_path,
                checksum,
                ..
            } = file_info;
            let pile = {
                if !files.contains_key(&pile_name) {
                    files.insert(pile_name.clone(), Pile::default());
                }
                files.get_mut(&pile_name).unwrap()
            };

            let pile_file = (pile_name, relative_path.clone());
            let checksum = checksum.expect("v1 Operation only stored files with checksums");
            if file_checksums.contains_key(&pile_file) {
                // Modified, Recreated, or Unchanged
                match file_checksums.get(&pile_file).unwrap() {
                    None => {
                        // Recreated
                        pile.created.insert(relative_path, checksum.clone());
                    }
                    Some(old_checksum) => {
                        // Modified or Unchanged
                        if old_checksum == &checksum {
                            pile.unmodified.insert(relative_path, checksum.clone());
                        } else {
                            pile.modified.insert(relative_path, checksum.clone());
                        }
                    }
                }
            } else {
                // Created
                pile.created.insert(relative_path, checksum.clone());
            }
            file_checksums.insert(pile_file.clone(), Some(checksum));
            these_files.insert(pile_file);
        }

        let deleted: HashMap<Option<String>, HashSet<PathBuf>> = file_set
            .difference(&these_files)
            .fold(HashMap::new(), |mut acc, (pile_name, rel_path)| {
                if !acc.contains_key(pile_name) {
                    acc.insert(pile_name.clone(), HashSet::new());
                }
                acc.get_mut(pile_name).unwrap().insert(rel_path.clone());
                acc
            });

        *file_set = these_files;

        for (pile_name, deleted) in deleted {
            if !files.contains_key(&pile_name) {
                files.insert(pile_name.clone(), Pile::default());
            }
            files.get_mut(&pile_name).unwrap().deleted = deleted;
        }

        let files = if is_anonymous {
            Hoard::Anonymous(files.remove(&None).unwrap_or_else(|| Pile {
                deleted: {
                    let mut map = HashSet::new();
                    map.insert(PathBuf::new());
                    map
                },
                ..Pile::default()
            }))
        } else {
            Hoard::Named(
                files
                    .into_iter()
                    .filter_map(|(name, pile)| name.map(|name| (name, pile)))
                    .collect(),
            )
        };

        Self {
            timestamp: old_v1.timestamp(),
            direction: old_v1.direction(),
            hoard: old_v1.hoard_name().to_string(),
            files,
            hoards_root: PathBuf::new(),
        }
    }
}

impl OperationImpl for OperationV2 {
    fn direction(&self) -> Direction {
        self.direction
    }

    fn contains_file(&self, pile_name: Option<&str>, rel_path: &Path, only_modified: bool) -> bool {
        self.files
            .get_pile(pile_name)
            .map_or(false, |pile| pile.contains_file(rel_path, only_modified))
    }

    fn timestamp(&self) -> OffsetDateTime {
        self.timestamp
    }

    fn hoard_name(&self) -> &str {
        &self.hoard
    }

    fn checksum_for(&self, pile_name: Option<&str>, rel_path: &Path) -> Option<Checksum> {
        self.files
            .get_pile(pile_name)
            .and_then(|pile| pile.checksum_for(rel_path))
    }

    fn all_files_with_checksums<'a>(&'a self) -> Box<dyn Iterator<Item = OperationFileInfo> + 'a> {
        match &self.files {
            Hoard::Anonymous(pile) => Box::new(pile.all_files_with_checksums().map(
                move |(path, checksum)| OperationFileInfo {
                    pile_name: None,
                    relative_path: path.to_path_buf(),
                    checksum,
                },
            )),
            Hoard::Named(piles) => Box::new(piles.iter().flat_map(move |(pile_name, pile)| {
                pile.all_files_with_checksums()
                    .map(move |(path, checksum)| OperationFileInfo {
                        pile_name: Some(pile_name.to_string()),
                        relative_path: path.to_path_buf(),
                        checksum,
                    })
            })),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(variant_size_differences)]
enum Hoard {
    Anonymous(Pile),
    Named(HashMap<String, Pile>),
}

impl Hoard {
    fn get_or_create_pile<'a>(
        map: &'a mut HashMap<String, Pile>,
        pile_name: Option<&str>,
    ) -> &'a mut Pile {
        let pile_name = pile_name.unwrap_or("");
        if !map.contains_key(pile_name) {
            map.insert(pile_name.to_string(), Pile::default());
        }
        map.get_mut(pile_name).unwrap()
    }

    fn require_checksum(checksum: Option<Checksum>, path: &Path) -> Result<Checksum, Error> {
        checksum.ok_or_else(|| {
            Error::IO(io::Error::new(
                io::ErrorKind::NotFound,
                format!("could not find {}", path.display()),
            ))
        })
    }

    fn checksum_type(hoard: &ConfigHoard, hoard_file: &HoardFile) -> ChecksumType {
        match (hoard, hoard_file.pile_name()) {
            (ConfigHoard::Anonymous(pile), None) => pile.config.checksum_type,
            (ConfigHoard::Named(piles), Some(pile_name)) => piles
                .piles
                .get(pile_name)
                .map(|pile| pile.config.checksum_type)
                .expect("provided pile name should always be in hoard"),
            (hoard, pile_name) => panic!(
                "mismatched hoard type and pile name option: hoard ({:?}), pile_name: {:?}",
                hoard, pile_name
            ),
        }
    }

    fn new(
        hoards_root: &Path,
        hoard_name: &str,
        hoard: &crate::hoard::Hoard,
        direction: Direction,
    ) -> Result<Self, Error> {
        let mut inner: HashMap<String, Pile> =
            OperationIter::new(hoards_root, hoard_name.to_string(), hoard, direction)?.fold(
                Ok(HashMap::new()),
                |acc, op| -> Result<HashMap<String, Pile>, Error> {
                    let mut acc = acc?;
                    let op = op?;

                    match op {
                        OperationType::Create(file) => {
                            let checksum = match direction {
                                Direction::Backup => Self::require_checksum(
                                    file.system_checksum(Self::checksum_type(hoard, &file))?,
                                    file.system_path(),
                                )?,
                                Direction::Restore => Self::require_checksum(
                                    file.hoard_checksum(Self::checksum_type(hoard, &file))?,
                                    file.hoard_path(),
                                )?,
                            };
                            Self::get_or_create_pile(&mut acc, file.pile_name())
                                .created
                                .insert(file.relative_path().to_path_buf(), checksum);
                        }
                        OperationType::Modify(file) => {
                            let checksum = match direction {
                                Direction::Backup => Self::require_checksum(
                                    file.system_checksum(Self::checksum_type(hoard, &file))?,
                                    file.system_path(),
                                )?,
                                Direction::Restore => Self::require_checksum(
                                    file.hoard_checksum(Self::checksum_type(hoard, &file))?,
                                    file.hoard_path(),
                                )?,
                            };
                            Self::get_or_create_pile(&mut acc, file.pile_name())
                                .modified
                                .insert(file.relative_path().to_path_buf(), checksum);
                        }
                        OperationType::Delete(file) => {
                            Self::get_or_create_pile(&mut acc, file.pile_name())
                                .deleted
                                .insert(file.relative_path().to_path_buf());
                        }
                        OperationType::Nothing(file) => {
                            let checksum = Self::require_checksum(
                                file.system_checksum(Self::checksum_type(hoard, &file))?,
                                file.system_path(),
                            )?;
                            Self::get_or_create_pile(&mut acc, file.pile_name())
                                .unmodified
                                .insert(file.relative_path().to_path_buf(), checksum);
                        }
                    }

                    Ok(acc)
                },
            )?;

        if inner.len() == 1 && inner.contains_key("") {
            Ok(Self::Anonymous(inner.remove("").unwrap()))
        } else {
            Ok(Self::Named(inner))
        }
    }

    fn get_pile(&self, name: Option<&str>) -> Option<&Pile> {
        match (name, self) {
            (None, Hoard::Anonymous(pile)) => Some(pile),
            (Some(name), Hoard::Named(piles)) => piles.get(name),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
struct Pile {
    created: HashMap<PathBuf, Checksum>,
    modified: HashMap<PathBuf, Checksum>,
    deleted: HashSet<PathBuf>,
    unmodified: HashMap<PathBuf, Checksum>,
}

impl Pile {
    fn contains_file(&self, rel_path: &Path, only_modified: bool) -> bool {
        self.created.contains_key(rel_path)
            || self.modified.contains_key(rel_path)
            || self.deleted.contains(rel_path)
            || (!only_modified && self.unmodified.contains_key(rel_path))
    }

    fn checksum_for(&self, rel_path: &Path) -> Option<Checksum> {
        self.created
            .get(rel_path)
            .or_else(|| self.modified.get(rel_path))
            .or_else(|| self.unmodified.get(rel_path))
            .map(Clone::clone)
    }

    fn all_files_with_checksums(&self) -> impl Iterator<Item = (&Path, Option<Checksum>)> {
        let created = self
            .created
            .iter()
            .map(|(path, checksum)| (path.as_path(), Some(checksum.clone())));
        let modified = self
            .modified
            .iter()
            .map(|(path, checksum)| (path.as_path(), Some(checksum.clone())));
        let unmodified = self
            .unmodified
            .iter()
            .map(|(path, checksum)| (path.as_path(), Some(checksum.clone())));
        let deleted = self.deleted.iter().map(|path| (path.as_path(), None));

        created.chain(modified).chain(unmodified).chain(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_test::{assert_tokens, Token};

    #[test]
    fn test_checksum_derives() {
        let checksum = Checksum::MD5("legit checksum".to_string());
        assert!(format!("{:?}", checksum).contains("MD5"));
        assert_eq!(checksum, checksum.clone());
        assert_tokens(
            &checksum,
            &[
                Token::Enum { name: "Checksum" },
                Token::Str("md5"),
                Token::Str("legit checksum"),
            ],
        );
    }

    mod v2_from_v1 {
        use super::super::super::v1;
        use super::*;
        use maplit;
        use time::Duration;

        fn assert_conversion(ops_v1: Vec<v1::OperationV1>, ops_v2: Vec<OperationV2>) {
            let mut mapping = HashMap::new();
            let mut file_set = HashSet::new();

            for (op_v1, op_v2) in ops_v1.into_iter().zip(ops_v2) {
                let new_op = OperationV2::from_v1(&mut mapping, &mut file_set, op_v1);
                assert_eq!(op_v2, new_op);
            }
        }

        #[test]
        fn test_from_anonymous_file() {
            let first_timestamp = time::OffsetDateTime::now_utc();
            let second_timestamp = first_timestamp - Duration::hours(2);
            let third_timestamp = second_timestamp - Duration::hours(2);
            let hoard_name = String::from("anon_file");
            let ops_v1 = vec![
                v1::OperationV1 {
                    timestamp: first_timestamp,
                    is_backup: true,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Anonymous(v1::Pile(
                        maplit::hashmap! { PathBuf::new() => String::from("d3369a026ace494f56ead54d502a00dd") },
                    )),
                },
                v1::OperationV1 {
                    timestamp: second_timestamp,
                    is_backup: false,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Anonymous(v1::Pile(
                        maplit::hashmap! { PathBuf::new() => String::from("d3369a026ace494f56ead54d502a00dd") },
                    )),
                },
                v1::OperationV1 {
                    timestamp: third_timestamp,
                    is_backup: true,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Anonymous(v1::Pile(HashMap::new())),
                },
            ];
            let ops_v2 = vec![
                OperationV2 {
                    timestamp: first_timestamp,
                    direction: Direction::Backup,
                    hoard: hoard_name.clone(),
                    hoards_root: PathBuf::new(),
                    files: Hoard::Anonymous(Pile {
                        created: maplit::hashmap! { PathBuf::new() => Checksum::MD5(String::from("d3369a026ace494f56ead54d502a00dd")) },
                        ..Pile::default()
                    }),
                },
                OperationV2 {
                    timestamp: second_timestamp,
                    direction: Direction::Restore,
                    hoard: hoard_name.clone(),
                    hoards_root: PathBuf::new(),
                    files: Hoard::Anonymous(Pile {
                        unmodified: maplit::hashmap! { PathBuf::new() => Checksum::MD5(String::from("d3369a026ace494f56ead54d502a00dd")) },
                        ..Pile::default()
                    }),
                },
                OperationV2 {
                    timestamp: third_timestamp,
                    direction: Direction::Backup,
                    hoard: hoard_name,
                    hoards_root: PathBuf::new(),
                    files: Hoard::Anonymous(Pile {
                        deleted: maplit::hashset! { PathBuf::new() },
                        ..Pile::default()
                    }),
                },
            ];

            assert_conversion(ops_v1, ops_v2);
        }

        #[test]
        fn test_from_anonymous_dir() {
            let first_timestamp = time::OffsetDateTime::now_utc();
            let second_timestamp = first_timestamp - Duration::hours(2);
            let third_timestamp = second_timestamp - Duration::hours(2);
            let hoard_name = String::from("anon_file");
            let ops_v1 = vec![
                v1::OperationV1 {
                    timestamp: first_timestamp,
                    is_backup: true,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Anonymous(v1::Pile(maplit::hashmap! {
                        PathBuf::from("file_1") => String::from("ba9d332813a722b273a95fa13dd88d94"),
                        PathBuf::from("file_2") => String::from("92ed3b5f07b44bc4f70d0b24d5e1867c"),
                    })),
                },
                v1::OperationV1 {
                    timestamp: second_timestamp,
                    is_backup: true,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Anonymous(v1::Pile(maplit::hashmap! {
                        PathBuf::from("file_1") => String::from("1cfab2a192005a9a8bdc69106b4627e2"),
                        PathBuf::from("file_2") => String::from("92ed3b5f07b44bc4f70d0b24d5e1867c"),
                        PathBuf::from("file_3") => String::from("797b373a9c4ec0d6de0a31a90b5bee8e"),
                    })),
                },
                v1::OperationV1 {
                    timestamp: third_timestamp,
                    is_backup: true,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Anonymous(v1::Pile(maplit::hashmap! {
                        PathBuf::from("file_1") => String::from("1cfab2a192005a9a8bdc69106b4627e2"),
                        PathBuf::from("file_3") => String::from("1deb21ef3bb87be4ad71d73fff6bb8ec"),
                    })),
                },
            ];
            let ops_v2 = vec![
                OperationV2 {
                    timestamp: first_timestamp,
                    direction: Direction::Backup,
                    hoard: hoard_name.clone(),
                    hoards_root: PathBuf::new(),
                    files: Hoard::Anonymous(Pile {
                        created: maplit::hashmap! {
                            PathBuf::from("file_1") => Checksum::MD5(String::from("ba9d332813a722b273a95fa13dd88d94")),
                            PathBuf::from("file_2") => Checksum::MD5(String::from("92ed3b5f07b44bc4f70d0b24d5e1867c")),
                        },
                        ..Pile::default()
                    }),
                },
                OperationV2 {
                    timestamp: second_timestamp,
                    direction: Direction::Backup,
                    hoard: hoard_name.clone(),
                    hoards_root: PathBuf::new(),
                    files: Hoard::Anonymous(Pile {
                        created: maplit::hashmap! {
                            PathBuf::from("file_3") => Checksum::MD5(String::from("797b373a9c4ec0d6de0a31a90b5bee8e"))
                        },
                        modified: maplit::hashmap! {
                            PathBuf::from("file_1") => Checksum::MD5(String::from("1cfab2a192005a9a8bdc69106b4627e2")),
                        },
                        unmodified: maplit::hashmap! {
                            PathBuf::from("file_2") => Checksum::MD5(String::from("92ed3b5f07b44bc4f70d0b24d5e1867c")),
                        },
                        ..Pile::default()
                    }),
                },
                OperationV2 {
                    timestamp: third_timestamp,
                    direction: Direction::Backup,
                    hoard: hoard_name,
                    hoards_root: PathBuf::new(),
                    files: Hoard::Anonymous(Pile {
                        modified: maplit::hashmap! {
                            PathBuf::from("file_3") => Checksum::MD5(String::from("1deb21ef3bb87be4ad71d73fff6bb8ec"))
                        },
                        deleted: maplit::hashset! { PathBuf::from("file_2") },
                        unmodified: maplit::hashmap! {
                            PathBuf::from("file_1") => Checksum::MD5(String::from("1cfab2a192005a9a8bdc69106b4627e2")),
                        },
                        ..Pile::default()
                    }),
                },
            ];

            assert_conversion(ops_v1, ops_v2);
        }

        #[test]
        #[allow(clippy::too_many_lines)]
        fn test_from_named() {
            let first_timestamp = time::OffsetDateTime::now_utc();
            let second_timestamp = first_timestamp - Duration::hours(2);
            let third_timestamp = second_timestamp - Duration::hours(2);
            let hoard_name = String::from("anon_file");
            let ops_v1 = vec![
                v1::OperationV1 {
                    timestamp: first_timestamp,
                    is_backup: true,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Named(maplit::hashmap! {
                        String::from("single_file") => v1::Pile(maplit::hashmap! { PathBuf::new() => String::from("d3369a026ace494f56ead54d502a00dd") }),
                        String::from("dir") => v1::Pile(maplit::hashmap! {
                            PathBuf::from("file_1") => String::from("ba9d332813a722b273a95fa13dd88d94"),
                            PathBuf::from("file_2") => String::from("92ed3b5f07b44bc4f70d0b24d5e1867c"),
                        })
                    }),
                },
                v1::OperationV1 {
                    timestamp: second_timestamp,
                    is_backup: true,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Named(maplit::hashmap! {
                        String::from("single_file") => v1::Pile(maplit::hashmap! { PathBuf::new() => String::from("d3369a026ace494f56ead54d502a00dd") }),
                        String::from("dir") => v1::Pile(maplit::hashmap! {
                            PathBuf::from("file_1") => String::from("1cfab2a192005a9a8bdc69106b4627e2"),
                            PathBuf::from("file_2") => String::from("92ed3b5f07b44bc4f70d0b24d5e1867c"),
                            PathBuf::from("file_3") => String::from("797b373a9c4ec0d6de0a31a90b5bee8e"),
                        })
                    }),
                },
                v1::OperationV1 {
                    timestamp: third_timestamp,
                    is_backup: true,
                    hoard_name: hoard_name.clone(),
                    hoard: v1::Hoard::Named(maplit::hashmap! {
                        String::from("single_file") => v1::Pile(HashMap::new()),
                        String::from("dir") => v1::Pile(maplit::hashmap! {
                            PathBuf::from("file_1") => String::from("1cfab2a192005a9a8bdc69106b4627e2"),
                            PathBuf::from("file_3") => String::from("1deb21ef3bb87be4ad71d73fff6bb8ec"),
                        })
                    }),
                },
            ];
            let ops_v2 = vec![
                OperationV2 {
                    timestamp: first_timestamp,
                    direction: Direction::Backup,
                    hoard: hoard_name.clone(),
                    hoards_root: PathBuf::new(),
                    files: Hoard::Named(maplit::hashmap! {
                        String::from("single_file") => Pile {
                            created: maplit::hashmap! { PathBuf::new() => Checksum::MD5(String::from("d3369a026ace494f56ead54d502a00dd")) },
                            .. Pile::default()
                        },
                        String::from("dir") => Pile {
                            created: maplit::hashmap! {
                                PathBuf::from("file_1") => Checksum::MD5(String::from("ba9d332813a722b273a95fa13dd88d94")),
                                PathBuf::from("file_2") => Checksum::MD5(String::from("92ed3b5f07b44bc4f70d0b24d5e1867c")),
                            },
                            .. Pile::default()
                        }
                    }),
                },
                OperationV2 {
                    timestamp: second_timestamp,
                    direction: Direction::Backup,
                    hoard: hoard_name.clone(),
                    hoards_root: PathBuf::new(),
                    files: Hoard::Named(maplit::hashmap! {
                        String::from("single_file") => Pile {
                            unmodified: maplit::hashmap! { PathBuf::new() => Checksum::MD5(String::from("d3369a026ace494f56ead54d502a00dd")) },
                            .. Pile::default()
                        },
                        String::from("dir") => Pile {
                            created: maplit::hashmap! {
                                PathBuf::from("file_3") => Checksum::MD5(String::from("797b373a9c4ec0d6de0a31a90b5bee8e"))
                            },
                            modified: maplit::hashmap! {
                                PathBuf::from("file_1") => Checksum::MD5(String::from("1cfab2a192005a9a8bdc69106b4627e2")),
                            },
                            unmodified: maplit::hashmap! {
                                PathBuf::from("file_2") => Checksum::MD5(String::from("92ed3b5f07b44bc4f70d0b24d5e1867c")),
                            },
                            .. Pile::default()
                        }
                    }),
                },
                OperationV2 {
                    timestamp: third_timestamp,
                    direction: Direction::Backup,
                    hoard: hoard_name,
                    hoards_root: PathBuf::new(),
                    files: Hoard::Named(maplit::hashmap! {
                        String::from("single_file") => Pile {
                            deleted: maplit::hashset! { PathBuf::new() },
                            .. Pile::default()
                        },
                        String::from("dir") => Pile {
                            modified: maplit::hashmap! {
                                PathBuf::from("file_3") => Checksum::MD5(String::from("1deb21ef3bb87be4ad71d73fff6bb8ec"))
                            },
                            deleted: maplit::hashset! { PathBuf::from("file_2") },
                            unmodified: maplit::hashmap! {
                                PathBuf::from("file_1") => Checksum::MD5(String::from("1cfab2a192005a9a8bdc69106b4627e2")),
                            },
                            .. Pile::default()
                        }
                    }),
                },
            ];

            assert_conversion(ops_v1, ops_v2);
        }
    }
}