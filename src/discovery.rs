use crate::domain::TestCase;
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

pub(crate) fn discover_tests(root: &Path) -> Result<Vec<TestCase>> {
    let mut pairs: BTreeMap<PathBuf, (Option<PathBuf>, Option<PathBuf>)> = BTreeMap::new();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.with_context(|| format!("walking {}", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.into_path();
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            continue;
        };
        if extension != "in" && extension != "sol" {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("strip test root from {}", path.display()))?;
        let mut key = relative.to_path_buf();
        key.set_extension("");
        let pair = pairs.entry(key).or_default();
        if extension == "in" {
            pair.0 = Some(path);
        } else {
            pair.1 = Some(path);
        }
    }

    Ok(pairs
        .into_iter()
        .map(|(key, (input_path, expected_path))| TestCase {
            key,
            input_path,
            expected_path,
        })
        .collect())
}
