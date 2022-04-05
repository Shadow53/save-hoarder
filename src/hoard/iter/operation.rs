use super::{HoardDiffIter, HoardItem};
use crate::hoard::iter::{DiffSource, HoardFileDiff};
use crate::hoard::{Direction, Hoard};
use crate::newtypes::HoardName;
use crate::paths::HoardPath;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub enum ItemOperation {
    Create(HoardItem),
    Modify(HoardItem),
    Delete(HoardItem),
    Nothing(HoardItem),
}

pub(crate) struct OperationIter {
    iterator: HoardDiffIter,
    direction: Direction,
}

impl OperationIter {
    pub(crate) fn new(
        hoards_root: &HoardPath,
        hoard_name: HoardName,
        hoard: &Hoard,
        direction: Direction,
    ) -> Result<Self, super::Error> {
        let iterator = HoardDiffIter::new(hoards_root, hoard_name, hoard)?;
        Ok(Self {
            iterator,
            direction,
        })
    }
}

impl Iterator for OperationIter {
    type Item = Result<ItemOperation, super::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        // For the purposes of this, Mixed counts for both local (backup) and remote (restore)
        // changes, and Unknown counts as a remote change.
        self.iterator.next().map(|diff| {
            tracing::trace!("found diff: {:?}", diff);
            #[allow(clippy::match_same_arms)]
            let op = match diff? {
                HoardFileDiff::BinaryModified { file, .. }
                | HoardFileDiff::TextModified { file, .. }
                | HoardFileDiff::PermissionsModified { file, .. } => ItemOperation::Modify(file),
                HoardFileDiff::Created {
                    file, diff_source, ..
                }
                | HoardFileDiff::Recreated {
                    file, diff_source, ..
                } => match (self.direction, diff_source) {
                    (_, DiffSource::Mixed) => ItemOperation::Create(file),
                    (Direction::Backup, DiffSource::Local) => ItemOperation::Create(file),
                    (Direction::Backup, DiffSource::Remote | DiffSource::Unknown) => {
                        ItemOperation::Delete(file)
                    }
                    (Direction::Restore, DiffSource::Remote | DiffSource::Unknown) => {
                        ItemOperation::Create(file)
                    }
                    (Direction::Restore, DiffSource::Local) => ItemOperation::Delete(file),
                },
                HoardFileDiff::Deleted {
                    file, diff_source, ..
                } => match (self.direction, diff_source) {
                    (_, DiffSource::Mixed) => ItemOperation::Delete(file),
                    (Direction::Backup, DiffSource::Local)
                    | (Direction::Restore, DiffSource::Remote | DiffSource::Unknown) => {
                        ItemOperation::Delete(file)
                    }
                    (Direction::Backup, DiffSource::Remote | DiffSource::Unknown)
                    | (Direction::Restore, DiffSource::Local) => ItemOperation::Create(file),
                },
                HoardFileDiff::Unchanged(file) => ItemOperation::Nothing(file),
            };
            Ok(op)
        })
    }
}
