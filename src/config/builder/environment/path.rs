//! See [`PathExists`].

use crate::paths::SystemPath;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::convert::{Infallible, TryInto};
use std::fmt;
use std::fmt::Formatter;
use crate::env_vars::PathWithEnv;

struct PathExistsVisitor;

impl<'de> de::Visitor<'de> for PathExistsVisitor {
    type Value = PathExists;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter.write_str("a path that may or may not contain environment variables")
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(self)
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        tracing::trace!("parsing path_exists item {}", s);
        let inner = PathWithEnv::from(s).process().ok();
        Ok(PathExists(inner))
    }
}

/// A conditional structure that tests whether or not the contained path exists.
///
/// The path can be anything from a file, directory, symbolic link, or otherwise, so long as
/// *something* with that name exists.
#[derive(Clone, PartialEq, Debug, Hash, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
#[allow(clippy::module_name_repetitions)]
pub struct PathExists(pub Option<SystemPath>);

impl<'de> Deserialize<'de> for PathExists {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_option(PathExistsVisitor)
    }
}

impl TryInto<bool> for PathExists {
    type Error = Infallible;

    fn try_into(self) -> Result<bool, Self::Error> {
        let PathExists(path) = self;
        match path {
            Some(path) => {
                tracing::trace!("checking if path \"{}\" exists", path.to_string_lossy());
                Ok(path.exists())
            }
            None => Ok(false),
        }
    }
}

impl fmt::Display for PathExists {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let PathExists(path) = self;
        write!(f, "PATH EXISTS {:?}", path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_test::{assert_de_tokens, assert_tokens, Token};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_none_is_false() {
        assert!(!TryInto::<bool>::try_into(PathExists(None)).expect("conversion should not fail"));
    }

    #[test]
    fn test_file_does_exist() {
        let temp = NamedTempFile::new().expect("failed to create temporary file");
        let exists: bool = PathExists(Some(
            SystemPath::try_from(temp.path().to_path_buf()).unwrap(),
        ))
        .try_into()
        .expect("failed to check if path exists");
        assert!(exists);
    }

    #[test]
    fn test_dir_does_exist() {
        let temp = tempdir().expect("failed to create temporary directory");
        let exists: bool = PathExists(Some(
            SystemPath::try_from(temp.path().to_path_buf()).unwrap(),
        ))
        .try_into()
        .expect("failed to check if path exists");
        assert!(exists);
    }

    #[test]
    fn test_file_does_not_exist() {
        let temp = NamedTempFile::new().expect("failed to create temporary file");
        fs::remove_file(temp.path()).expect("failed to remove temporary file");
        let exists: bool = PathExists(Some(
            SystemPath::try_from(temp.path().to_path_buf()).unwrap(),
        ))
        .try_into()
        .expect("failed to check if path exists");
        assert!(!exists);
    }

    #[test]
    fn test_dir_does_not_exist() {
        let temp = tempdir().expect("failed to create temporary directory");
        fs::remove_dir(temp.path()).expect("failed to remove temporary directory");
        let exists: bool = PathExists(Some(
            SystemPath::try_from(temp.path().to_path_buf()).unwrap(),
        ))
        .try_into()
        .expect("failed to check if path exists");
        assert!(!exists);
    }

    #[test]
    fn test_custom_deserialize() {
        let path_str = "/test/path/example";
        let path = PathExists(Some(SystemPath::try_from(PathBuf::from(path_str)).unwrap()));
        assert_tokens(&path, &[Token::Some, Token::Str(path_str)]);
    }

    #[test]
    fn test_env_is_expanded_in_path() {
        std::env::set_var("HOARD_TEST_ENV", "hoard-test");
        let path_with_env = "/test/path/${HOARD_TEST_ENV}/leaf";
        let path_resolved = "/test/path/hoard-test/leaf";
        let path = PathExists(Some(
            SystemPath::try_from(PathBuf::from(path_resolved)).unwrap(),
        ));
        assert_de_tokens(&path, &[Token::Str(path_with_env)]);
    }
}
