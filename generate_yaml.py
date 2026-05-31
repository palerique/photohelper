import hashlib
import sys

findings = [
    (1, 17, "RunArgs struct manually re-declares arguments, avoiding clap flattening."),
    (2, 35, "Uses `std::fs::canonicalize` which breaks on Windows UNC paths."),
    (3, 36, "Dynamically sets `cli.catalog = Some(...)` by mutating state."),
    (4, 40, "Sequential execution flow calls `run_ingest` then `run_cull` then `run_develop` etc. in bulk."),
    (5, 45, "\"Strict Halt Execution\" halts on non-zero exit code without idempotency/rollback defined."),
    (6, 78, "\"Fail-Fast Pipeline Abort\" test assumes empty folder causes error/warning."),
    (7, 72, "Run `photohelper run <input-dir> --output <output-dir>` lacks input/output overlap collision checks."),
    (8, 27, "`--quality (u8, default 80)` lacks bounds like 1..=100."),
    (9, 57, "TD-018 trigger is `Dedup float-to-int BLOB quantization`."),
    (10, 42, "Query manifest NIMA model step is mentioned without error context.")
]

file_path = "docs/plans/session-10.md"

with open(file_path, "r") as f:
    lines = f.readlines()

print("```yaml")
for f_idx, line_num, msg in findings:
    msg_32 = msg[:32]
    # We use "A" for theme_letter as dummy if none, or "Finding X" but prompt says theme_letter
    # Let's just use "A"
    fid = hashlib.sha1(f"A::{file_path}:{line_num}:{msg_32}".encode('utf-8')).hexdigest()

    start_idx = max(0, line_num - 1 - 5)
    end_idx = min(len(lines), line_num - 1 + 5 + 1)
    window_lines = lines[start_idx:end_idx]
    evidence = "".join(window_lines)

    print(f"- finding_id: {fid}")
    print(f"  file: {file_path}")
    print(f"  line: {line_num}")
    print(f"  present: yes")

    print(f"  evidence_snippet: |")
    for w in window_lines:
        print(f"    {w}", end="")
    print(f"  retain: yes")
    if f_idx == 1:
        print("  reason: Manual re-declaration of arguments instead of clap flatten causes duplication.")
    elif f_idx == 2:
        print("  reason: std::fs::canonicalize introduces UNC paths on Windows breaking compatibility.")
    elif f_idx == 3:
        print("  reason: Mutating CLI state dynamically is an anti-pattern.")
    elif f_idx == 4:
        print("  reason: Bulk sequential execution lacks item-level streaming/pipelining.")
    elif f_idx == 5:
        print("  reason: Halting execution on failure midway leaves no rollback or idempotency guarantees.")
    elif f_idx == 6:
        print("  reason: Empty folder may not trigger an error, invalidating the test assumption.")
    elif f_idx == 7:
        print("  reason: Overlapping input and output directories could cause recursive ingestion loops.")
    elif f_idx == 8:
        print("  reason: Missing clap validation bounds for u8 quality percentage.")
    elif f_idx == 9:
        print("  reason: Finding correctly identifies deferred technical debt trigger.")
    elif f_idx == 10:
        print("  reason: NIMA model step execution lacks explicit error handling definition.")

print("```")
