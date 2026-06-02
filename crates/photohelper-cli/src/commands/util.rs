//! Shared CLI utilities: name formatting, collision resolution, temp-file RAII.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Map a NIMA score to a (rating, tier_label) pair.
pub fn nima_score_to_rating_and_tier(score: f32) -> (i32, &'static str) {
    if score < 4.0 {
        (1, "discard")
    } else if score < 5.5 {
        (2, "poor")
    } else if score < 7.0 {
        (3, "fair")
    } else if score < 8.5 {
        (4, "good")
    } else {
        (5, "excellent")
    }
}

/// Format a NIMA score as a zero-padded 5-character label (`{:05.2}`).
pub fn format_nima_score_label(score: f32) -> String {
    format!("{score:05.2}")
}

/// RAII guard that deletes a temporary file on drop unless `commit()` is called.
pub struct TempFileGuard {
    path: PathBuf,
    committed: bool,
}

impl TempFileGuard {
    /// Create a new guard for `path`. The file is NOT created by this call.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    /// Mark the file as committed; `drop` will not delete it.
    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if !self.committed {
            if let Err(e) = std::fs::remove_file(&self.path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        path = %self.path.display(),
                        error = %e,
                        "failed to clean up temporary file"
                    );
                }
            }
        }
    }
}

/// Build a deterministic collision-free mapping from source paths to output filenames.
///
/// `output_dir` is the directory that will receive the outputs.
/// `items` provides the source paths.
/// `name_fn` maps a source path to a candidate output filename (including extension).
///
/// When two sources map to the same candidate filename, the second and subsequent
/// ones receive a `_N` suffix inserted before the extension (e.g. `photo_1.jpg`).
///
/// On macOS and Windows the collision key is lowercased to match the
/// case-insensitive filesystem semantics.
pub fn resolve_collisions<'a, I, F>(
    output_dir: &Path,
    items: I,
    name_fn: F,
) -> HashMap<PathBuf, PathBuf>
where
    I: IntoIterator<Item = &'a PathBuf>,
    F: Fn(&Path) -> String,
{
    let mut result: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut counts: HashMap<PathBuf, usize> = HashMap::new();

    for src in items {
        let base = name_fn(src.as_path());
        let candidate = output_dir.join(&base);

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let key = PathBuf::from(candidate.to_string_lossy().to_lowercase());
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let key = candidate.clone();

        let count = counts.entry(key).or_insert(0);
        let final_path = if *count > 0 {
            // Insert `_N` before the extension.
            let ext_pos = base.rfind('.').unwrap_or(base.len());
            let stem = &base[..ext_pos];
            let ext = &base[ext_pos..];
            output_dir.join(format!("{stem}_{count}{ext}"))
        } else {
            candidate
        };
        *count += 1;
        result.insert(src.clone(), final_path);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_nima_score_label() {
        assert_eq!(format_nima_score_label(9.5), "09.50");
        assert_eq!(format_nima_score_label(10.0), "10.00");
        assert_eq!(format_nima_score_label(3.1425), "03.14");
    }

    #[test]
    fn test_resolve_collisions_no_collision() {
        let dir = PathBuf::from("/out");
        let items: Vec<PathBuf> = vec![PathBuf::from("/a/foo.jpg"), PathBuf::from("/b/bar.jpg")];
        let map = resolve_collisions(&dir, &items, |p| {
            p.file_name().unwrap().to_string_lossy().into_owned()
        });
        assert_eq!(map[&items[0]], dir.join("foo.jpg"));
        assert_eq!(map[&items[1]], dir.join("bar.jpg"));
    }

    #[test]
    fn test_resolve_collisions_with_collision() {
        let dir = PathBuf::from("/out");
        let items: Vec<PathBuf> = vec![
            PathBuf::from("/a/photo.jpg"),
            PathBuf::from("/b/photo.jpg"),
            PathBuf::from("/c/photo.jpg"),
        ];
        let map = resolve_collisions(&dir, &items, |_p| "photo.jpg".to_string());
        assert_eq!(map[&items[0]], dir.join("photo.jpg"));
        assert_eq!(map[&items[1]], dir.join("photo_1.jpg"));
        assert_eq!(map[&items[2]], dir.join("photo_2.jpg"));
    }
}
