use std::fmt::Debug;
use std::path::Path;
use tokio::fs;

pub use ::toml::*;
use serde::de::DeserializeOwned;

pub async fn assert_file_contains_deserializable<T>(path: &Path, expected: &T)
where
    T: PartialEq + Debug + DeserializeOwned,
{
    let content_str = fs::read_to_string(path).await.unwrap_or_else(|err| {
        panic!(
            "failed to read from file at {}: {}",
            path.to_string_lossy(),
            err
        )
    });

    let content: T = from_str(&content_str).expect("failed to deserialize file contents");

    assert_eq!(
        expected, &content,
        "file contents do not match expected contents\nDeserialized from: {content_str}"
    );
}
