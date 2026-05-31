import hashlib

def get_sha1(theme_letter, file, line, msg):
    # first 32 chars of msg
    msg_32 = msg[:32]
    content = f"{theme_letter}::{file}:{line}:{msg_32}"
    return hashlib.sha1(content.encode('utf-8')).hexdigest()

findings = [
    {
        "theme": "A",
        "file": "docs/plans/session-14.md",
        "line": 30,
        "msg": "Deleting corrupted file before calling write_xmp creates a TOCTOU race condition.",
        "present": "yes",
        "evidence_snippet": "- In `crates/photohelper-sidecar/src/conflict.rs`, instruct `ForceOverwrite` to simply delete the corrupted sidecar file from disk before calling `write_xmp` as a creation operation, avoiding complex fallback flags.",
        "retain": "yes",
        "reason": "Valid finding; deleting and then creating file introduces a race condition."
    },
    {
        "theme": "B",
        "file": "docs/plans/session-14.md",
        "line": 27,
        "msg": ".unwrap() on .parent() will panic if the path is a bare filename or root directory.",
        "present": "yes",
        "evidence_snippet": "- Use `tempfile::Builder::new().tempfile_in(target_path.parent().unwrap())` instead of `/tmp` to avoid `EXDEV` errors across filesystems, and ensure temporary file cleanup automatically on parse/write errors, preventing resource leaks.",
        "retain": "yes",
        "reason": "Valid finding; unwrap on parent is unsafe and can panic."
    },
    {
        "theme": "C",
        "file": "docs/plans/session-14.md",
        "line": 19,
        "msg": "If `rdf:Description` itself is self-closing, injecting children requires mutating it to `Event::Start` and adding `Event::End`. Missing test for this at line 58.",
        "present": "yes",
        "evidence_snippet": "- **Dropping Elements safely**: Handle nested `rdf:Description` safely by tracking depth. Handle `Event::Empty` tags correctly: if a dropped tag is self-closing, do NOT enter the `Dropping` state. If inside `Dropping`, ignore `Event::Empty` without altering the depth counter.",
        "retain": "yes",
        "reason": "Valid finding; missing logic to handle child injection in self-closing rdf:Description."
    },
    {
        "theme": "D",
        "file": "docs/plans/session-14.md",
        "line": 23,
        "msg": "`BTreeMap` reorders attributes alphabetically, contradicting goal of preserving original order and introducing testing complexity (line 62).",
        "present": "yes",
        "evidence_snippet": "- **Attributes Structure**: Use `std::collections::BTreeMap` for buffering existing attributes to structurally guarantee no duplicate attributes during injection.",
        "retain": "yes",
        "reason": "Valid finding; BTreeMap sorting will disrupt original attribute ordering."
    },
    {
        "theme": "E",
        "file": "docs/plans/session-14.md",
        "line": 18,
        "msg": "`InsideDescription` fails to track depth of unmanaged tags. Missing EOF injection fallback. Undefined behavior on invalid state machine transitions.",
        "present": "yes",
        "evidence_snippet": "- **Unified State Machine**: Use a unified, comprehensive `WriterState` enum (e.g., `SeekingDescription`, `InsideDescription`, `Dropping { depth: std::num::NonZeroUsize }`, `InjectionComplete`) to avoid fragmented boolean flags. Use `NonZeroUsize` for depth to structurally prevent underflow panics.",
        "retain": "yes",
        "reason": "Valid finding; InsideDescription is missing depth tracking."
    },
    {
        "theme": "F",
        "file": "docs/plans/session-14.md",
        "line": 36,
        "msg": "`conflict.rs` docstrings mismatch. `writer.rs` docstring update missing removing hardcoded tempfile UUID format. Script update already exists in codebase.",
        "present": "yes",
        "evidence_snippet": "- **writer.rs** & **conflict.rs**: Completely delete the old `render_xmp` function and its stale `# Stop-gap` docstring. Rewrite `write_xmp`'s docstring using \"stream-based\" terminology to accurately describe its non-destructive merge behavior. Ensure `conflict.rs` docstrings are updated to reflect the new overwrite/fallback behavior.",
        "retain": "yes",
        "reason": "Valid finding; docstring requirements are incomplete or conflicting."
    },
    {
        "theme": "G",
        "file": "docs/plans/session-14.md",
        "line": 18,
        "msg": "`NonZeroUsize` does not structurally prevent underflow panics and is overly complex.",
        "present": "yes",
        "evidence_snippet": "- **Unified State Machine**: Use a unified, comprehensive `WriterState` enum (e.g., `SeekingDescription`, `InsideDescription`, `Dropping { depth: std::num::NonZeroUsize }`, `InjectionComplete`) to avoid fragmented boolean flags. Use `NonZeroUsize` for depth to structurally prevent underflow panics.",
        "retain": "yes",
        "reason": "Valid finding; NonZeroUsize does not prevent underflow and adds complexity."
    },
    {
        "theme": "H",
        "file": "docs/plans/session-14.md",
        "line": 29,
        "msg": "Ambiguous error handling on `File::open`. Must only fall back on `NotFound`. Missing error context on `create_dir_all`.",
        "present": "yes",
        "evidence_snippet": "- Rely on standard `std::fs::File::open` behavior. If the file doesn't exist, we fall back to Creation. Ensure the target directory exists (`std::fs::create_dir_all`) *before* attempting creation, rather than duplicating OS structural checks.",
        "retain": "yes",
        "reason": "Valid finding; must be specific about falling back only on NotFound."
    },
]

print("```yaml")
for f in findings:
    finding_id = get_sha1(f['theme'], f['file'], f['line'], f['msg'])
    print(f"- finding_id: {finding_id}")
    print(f"  file: {f['file']}")
    print(f"  line: {f['line']}")
    print(f"  present: {f['present']}")
    print(f"  evidence_snippet: >-\n    {f['evidence_snippet']}")
    print(f"  retain: {f['retain']}")
    print(f"  reason: {f['reason']}")
print("```")
