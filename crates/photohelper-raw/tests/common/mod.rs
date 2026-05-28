//! Shared test infrastructure for `photohelper-raw` integration tests.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use std::io::Read;
use std::path::{Path, PathBuf};

/// Magic bytes a Git LFS pointer file starts with. If a "CR3 fixture"
/// is actually a pointer (`git lfs install` was skipped on this
/// machine), tests panic with an actionable message rather than
/// silently passing on synthetic data.
const LFS_POINTER_MAGIC: &[u8] = b"version https://git-lfs";

/// Per plan §Deliverable 3 PR1-T13: verify that the named CR3 fixture
/// is the real CR3 binary (not the LFS pointer). Returns the canonical
/// path on success; panics with an actionable message on failure so the
/// developer immediately sees what went wrong.
pub fn fixture_is_real_cr3(path: &Path) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or_else(|e| {
        panic!(
            "fixture_is_real_cr3: cannot canonicalize {} ({e}). \
             Run `git lfs install` and `git lfs pull` then retry.",
            path.display()
        );
    });

    let metadata = std::fs::metadata(&canonical).unwrap_or_else(|e| {
        panic!(
            "fixture_is_real_cr3: cannot stat {} ({e})",
            canonical.display()
        );
    });

    let size = metadata.len();
    assert!(
        size >= 1024 * 1024,
        "fixture_is_real_cr3: {} is {size} bytes; real CR3s are >1 MB. \
         Either this is the Git LFS pointer (run `git lfs install && \
         git lfs pull`) or a corrupt download.",
        canonical.display()
    );

    let mut head = [0u8; 23];
    let mut f = std::fs::File::open(&canonical).expect("open fixture");
    let n = f.read(&mut head).expect("read fixture prefix");
    assert!(
        !(n >= LFS_POINTER_MAGIC.len() && head.starts_with(LFS_POINTER_MAGIC)),
        "fixture_is_real_cr3: {} starts with the Git LFS pointer magic. \
         Run `git lfs install && git lfs pull` to fetch the real binary.",
        canonical.display()
    );

    canonical
}

/// Path to a CR3 fixture by filename. Returns the workspace-relative
/// `tests/fixtures/cr3/<name>` path.
pub fn fixture_path(name: &str) -> PathBuf {
    // CARGO_MANIFEST_DIR resolves to `crates/photohelper-raw/`; the
    // workspace root is two parents up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/cr3")
        .join(name)
}
