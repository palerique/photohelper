import hashlib

findings = [
    (1, "crates/photohelper-cli/src/commands/develop.rs", 430, "Strict mode fail-fast logic doesn't cover all I/O errors or validation errors. When `merge_and_write` or validation fails, it logs but doesn't set `cancelled.store(true)`."),
    (2, "crates/photohelper-cli/src/commands/develop.rs", 415, "The `row.dedup_cluster_id()` isn't filtered for negatives before being injected into `lr_keywords` flat/hierarchical sets."),
    (3, "crates/photohelper-cli/src/commands/develop.rs", 377, "The manual match on `rating_num` uses a dangerous wildcard fallback `_ => photohelper_sidecar::Rating::Five` which assigns 5-stars to garbage photos."),
    (4, "crates/photohelper-cli/src/commands/develop.rs", 303, "`DevelopStats` struct does not encapsulate its core invariant `walked == sum(outcomes)`, `walked` is incremented early."),
    (5, "crates/photohelper-cli/src/commands/develop.rs", 221, "Lossy String Deduplication `sidecar_path.to_string_lossy().to_lowercase()` causes collisions and ignores FAT32 case-insensitivity on Linux."),
    (6, "crates/photohelper-cli/src/commands/develop.rs", 117, "Duplicated XML 1.0 character validation abstraction `is_valid_xml_string` exists here and in `photohelper-sidecar/src/writer.rs:207`."),
    (7, "crates/photohelper-sidecar/src/conflict.rs", 57, "TOCTOU Vulnerability: `path.exists()` is checked prior to invoking `read_xmp(path)`."),
    (8, "SESSION-STATE.md", 10, "Workspace Ledger Desync. Still claims `Current session: 10`."),
    (9, "crates/photohelper-sidecar/src/lib.rs", 591, "The test `write_xmp_atomic_no_partial_on_io_error` only tests directory resolution, not actual IO write failures."),
    (10, "TECH-DEBT.md", 1, "Lost Deferral for Primitive Obsession across measurement domains."),
    (11, "crates/photohelper-cli/src/commands/develop.rs", 466, "Misattributed Error Context Path: `tracing::warn!(path = %source_path.display(), ...)` uses the `.CR3` path instead of `sidecar_path`."),
    (12, "crates/photohelper-sidecar/src/conflict.rs", 61, "Broad catch block on `--force` fallback swallows all `read_xmp` errors indiscriminately."),
    (13, "crates/photohelper-cli/src/commands/develop.rs", 135, "Factual inaccuracy: `# Errors` omits validation failures."),
    (14, "crates/photohelper-sidecar/src/writer.rs", 119, "`let _ = write!(...)` drops result silently in violation of CLAUDE.md."),
    (15, "crates/photohelper-sidecar/src/writer.rs", 41, "Loss of temporary file context in Atomic Write IO Error."),
    (16, "crates/photohelper-sidecar/src/conflict.rs", 55, "Boolean blindness in `merge_and_write` accepting `force: bool`."),
    (17, "crates/photohelper-sidecar/src/conflict.rs", 155, "Untested Error-Handling State Branch in Conflict Resolution for `(None, Some(_))`."),
    (18, "crates/photohelper-cli/src/commands/develop.rs", 338, "Deep nesting and redundant `!score.is_nan()` check in NIMA Validation."),
    (19, "crates/photohelper-cli/src/commands/develop.rs", 316, "Repetitive CLI Arguments Builder Population."),
    (20, "crates/photohelper-sidecar/src/conflict.rs", 159, "Duplicated Ownership Calculation in Conflict Resolver (`is_ours`).")
]

snippets = {
    1: """            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "invalid settings; skipping");
                stats.errored.fetch_add(1, Ordering::Relaxed);
                return;
            }""",
    2: """            if let Some(cluster_id) = row.dedup_cluster_id() {
                flat.insert(format!("cluster:{cluster_id}"));
                hierarchical.insert(format!("photohelper|cluster:{cluster_id}"));
            }""",
    3: """                let rating = match rating_num {
                    1 => photohelper_sidecar::Rating::One,
                    2 => photohelper_sidecar::Rating::Two,
                    3 => photohelper_sidecar::Rating::Three,
                    4 => photohelper_sidecar::Rating::Four,
                    _ => photohelper_sidecar::Rating::Five,
                };""",
    4: """        stats.walked.fetch_add(1, Ordering::Relaxed);
        let source_path = row.source_path();

        // Step a: existence pre-check.
        if !source_path.exists() {""",
    5: """        // On case-insensitive filesystems (macOS, Windows), normalize path casing for deduplication
        // to prevent duplicate rows targeting the same sidecar from causing concurrent write races.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let dedup_key = PathBuf::from(sidecar_path.to_string_lossy().to_lowercase());""",
    6: """fn is_valid_xml_string(s: &str) -> bool {
    s.chars().all(|c| {
        let val = c as u32;""",
    7: """    if force {
        let to_write = if path.exists() {
            match read_xmp(path) {""",
    8: """**Last session**: 9 (`lightroom-sync-fixes` — 2026-05-31) — **SHIPPED** via PR #12. Implemented BUG-001 fixes including smart CLI warnings/shorthands (`--all-lr`), upfront validation for distinct and XML-safe custom color labels, high-performance granular conflict logging, `mtime`-based conflict shield, and precision `mtime` alignment. Session-end R1 (0C+1H+2M+2L; 5 resolved) → R2 CLEAN (0 findings).

**Current session**: 10 (`run-pipeline` — 2026-05-31) — branch `session-10/run-pipeline`. **PLANNED** (Implementing the orchestrating `run` subcommand).""",
    9: """    #[test]
    fn write_xmp_atomic_no_partial_on_io_error() {
        // Use a path in a non-existent directory — will fail at File::create.
        let raw_p = Path::new("/nonexistent/path/photo.xmp");""",
    10: """# photohelper — Tech-Debt Ledger

> Known shortcuts taken for velocity, each with a remediation plan and a
> **binding trigger**. This ledger is the canonical view of "where the codebase""",
    11: """            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "XMP write failed");
                stats.errored.fetch_add(1, Ordering::Relaxed);
            }""",
    12: """            match read_xmp(path) {
                Ok(existing) => existing.merge(incoming),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "force: failed to read existing XMP; falling back to direct write");
                    incoming.clone()
                }
            }""",
    13: """/// # Errors
///
/// Returns `Err` only for fatal setup failures (catalog open, photo query, heartbeat spawn).""",
    14: """    if let Some(t) = settings.temperature() {
        let _ = write!(attrs, "\\n      crs:Temperature=\\"{t}\\"");
    }""",
    15: """    let tmp_path = path.with_extension(format!("phdev.{pid}.{nonce}.tmp"));

    // Write to temp file.
    let mut f = std::fs::File::create(&tmp_path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;""",
    16: """pub fn merge_and_write(
    path: &SidecarPath,
    incoming: &SidecarSettings,
    force: bool,
) -> Result<WriteOutcome, Error> {""",
    17: """            (None, Some(_)) => {
                // Existing sidecar has ph:LastProcessedAt (photohelper-written) but no
                // xmp:MetadataDate — if we own it, we can safely update it. Otherwise,""",
    18: """        // Validate early that the NIMA score is finite
        let valid_nima = if let Some(score) = row.nima_score() {
            if score.is_finite() && !score.is_nan() {""",
    19: """        // Step c: build per-photo settings (fresh builder each photo).
        let mut builder = SidecarSettings::builder();
        if let Some(v) = cli_exposure {
            builder = builder.exposure(v);
        }""",
    20: """                // xmp:MetadataDate — if we own it, we can safely update it. Otherwise,
                // conservatively preserve; the absence of a date is ambiguous.
                let is_ours = existing.photohelper_id().is_some()
                    && existing.photohelper_id() == incoming.photohelper_id();"""
}

present = {i: "yes" for i in range(1, 21)}
present[10] = "no"

retains = {
    1: "yes", 2: "yes", 3: "yes", 4: "yes", 5: "yes", 6: "yes", 7: "yes",
    8: "yes", 9: "yes", 10: "no", 11: "yes", 12: "yes", 13: "yes", 14: "yes",
    15: "yes", 16: "yes", 17: "yes", 18: "yes", 19: "yes", 20: "yes"
}

reasons = {
    1: "Validation error logs but fails to cancel walk if strict mode is active.",
    2: "dedup_cluster_id negative values are not filtered before being formatted.",
    3: "Wildcard fallback incorrectly assumes everything unmapped is 5-stars.",
    4: "Early increment breaks core invariant if pre-checks fail.",
    5: "Lossy string deduplication ignores true filesystem identity semantics.",
    6: "Duplicate XML validation logic between develop and sidecar writer.",
    7: "path.exists check introduces race condition before read_xmp.",
    8: "Ledger out of sync with actual session.",
    9: "Test does not trigger actual I/O writes, only directory creation failure.",
    10: "No evidence of primitive obsession deferral in TECH-DEBT.md.",
    11: "Error context logs source CR3 instead of the target sidecar path.",
    12: "Catch-all error handler swallows meaningful read failures.",
    13: "Doc-comment neglects to mention errors surfaced by settings builder.",
    14: "fmt::Write result dropped silently without unwrapping or error propagation.",
    15: "Atomic write failure error returns the destination path instead of the failing temp path.",
    16: "Boolean flag parameter obscures intent compared to enum.",
    17: "Test suite lacks explicit coverage for the (None, Some(_)) state.",
    18: "is_nan is redundant since is_finite already excludes NaN.",
    19: "Repetitive builder population for arguments.",
    20: "is_ours logic duplicated in multiple match arms."
}

def format_multiline(s):
    lines = s.split('\\n')
    res = "|\\n"
    for line in lines:
        res += "    " + line + "\\n"
    return res

out = []
for f in findings:
    i, file, line, msg = f
    msg_32 = msg[:32]
    sha_input = f"::{file}:{line}:{msg_32}"
    finding_id = hashlib.sha1(sha_input.encode('utf-8')).hexdigest()

    out.append(f"- finding_id: {finding_id}")
    out.append(f"  file: {file}")
    out.append(f"  line: {line}")
    out.append(f"  present: {present[i]}")
    out.append(f"  evidence_snippet: {format_multiline(snippets[i])}".rstrip())
    out.append(f"  retain: {retains[i]}")
    out.append(f"  reason: {reasons[i]}")

print("\\n".join(out))
