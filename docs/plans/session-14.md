# Session 14 Plan — TD-022: XMP Sidecar I/O Proper Library (Preserve External Fields)
**Branch**: `session-14/xmp-library-upgrade`
**Date**: 2026-05-31
**Status**: v4 — completed

---

## Session goal

Implement TD-022: Replace the hand-rolled `quick-xml` XMP writer template in `crates/photohelper-sidecar/src/writer.rs` with a robust event-based pass-through writer. The new writer will preserve all third-party XMP fields (like Lightroom's `crs:CameraProfile` or `crs:ToneCurvePV2012`) across develop overwrites, while safely applying photohelper's managed settings (NIMA scores, ratings, tags, basic tone adjustments).

---

## What will exist by end of session

### 1. State-Machine XML Pass-through
- **Create vs Update Split**: Define completely distinct structural paths for sidecar creation vs update. Bypass stream parsing entirely for creation: write the XML skeleton directly (including mandatory standard XMP `xpacket` processing instructions, `x:xmpmeta`, and `rdf:RDF`/`rdf:Description`).
- **Unified State Machine**: Use a comprehensive `WriterState` enum (e.g., `SeekingDescription { depth: usize }`, `InsideDescription { unmanaged_depth: usize }`, `InjectionComplete`). Maintain a separate `drop_depth: usize` variable outside the state machine to track dropped tags without losing the `unmanaged_depth` context. Expand `SeekingDescription` to explicitly track its depth within `rdf:RDF`, ensuring `<rdf:Description>` is only intercepted when `depth == 0` (direct child).
- **Dropping Elements safely**: Use an early-return guard clause (`if drop_depth > 0 { ... }`) to handle drops cleanly. If a dropped tag is self-closing, do NOT alter the drop depth. If the target `rdf:Description` itself is intercepted as `Event::Empty`, it MUST be explicitly converted into `Event::Start`, children injected, and then closed with `Event::End`.

### 2. Attribute deduplication and Strict Injection
- The `rdf:Description` element will be intercepted exactly once. If `rdf:Description` is missing, inject it directly *before* the `</rdf:RDF>` closing tag (triggering `InjectionComplete`) rather than relying on stream termination.
- **Attributes Structure**: Use a `Vec` to collect attributes. Filter out updated or cleared keys. Explicitly track if required namespaces (e.g., `xmlns:ph`) are present during the filter pass, and conditionally append them at the end only if missing, preventing structurally malformed duplicate namespaces.
- **Borrow Checker Safety**: Allocate an owned tag (`BytesStart::owned_name()`) and push attributes from the old tag + new managed tags to satisfy the borrow checker. Handle attribute clearing (fields set to `Update::Clear`).

### 3. Fail-Safe Error Handling
- Use `tempfile::Builder::new().tempfile_in(target_path.canonicalize().unwrap_or_else(|_| target_path.clone()).parent().unwrap_or_else(|| std::path::Path::new(".")))` instead of `/tmp` to guarantee same-mount tempfiles avoiding `EXDEV` errors. Ensure temporary file cleanup automatically on parse/write errors, preventing resource leaks.
- Return explicit context in errors (`Error::XmlParse` should wrap the underlying `quick_xml::Error` and carry current `WriterState`). Enforce a strict termination invariant: if EOF is reached and state is not `InjectionComplete`, strictly return `Error::MissingRdfDescription` (or `Error::XmlParse` for truncated streams) and abort. Do NOT fall back to Creation on EOF to avoid silent data loss.
- Rely on standard `std::fs::File::open` behavior. If the file doesn't exist, we fall back to Creation ONLY if the error is `std::io::ErrorKind::NotFound`. Ensure the target directory exists (`std::fs::create_dir_all`) *before* attempting creation, mapping any errors with full path context.
- In `crates/photohelper-sidecar/src/conflict.rs`, `ForceOverwrite` MUST NOT delete the corrupted file first (which causes a TOCTOU race condition and data-loss). Instead, instruct `write_xmp` to bypass stream parsing (like Creation) and rely on `tempfile::NamedTempFile::persist` to atomically overwrite the corrupted target.

### 4. Ledger & Doc Sync (TD-022 Closure)
- **SESSION-STATE.md**: Increment session state and update the component progress table/dependency matrix to reflect the new TD-022 XMP capability.
- **TECH-DEBT.md**: Move `TD-022` to the closed section with rationale.
- **User Docs**: Update `README.md` and/or `docs/user-guide/lightroom-sync.md` to document the non-destructive preservation capability and reflect the removal of the wrapper script CLI flags (`--auto-tone` / `--lr-label-score`).
- **writer.rs**, **conflict.rs**, **lib.rs**: Delete `render_xmp`. Rewrite `write_xmp`'s docstring for merge behavior (removing `.tmp` claims) and clarify that namespaces are preserved as-is on updates. Update `conflict.rs` docstrings to fix the `strategy` parameter, match the `2.1s` margin, delete transient "(Theme X fix)" notes, remove the `--force` CLI leak, and replace "crs: settings" with "XMP settings". Update `lib.rs` to assert full third-party XMP support.

---

## What is explicitly OUT OF SCOPE

- Modifying `reader.rs` to build a full DOM tree. We will keep `reader.rs` as a fast, lenient event stream parser.
- Adding C++ dependencies (e.g. `xmp-toolkit`). We will use pure Rust `quick-xml` event streaming.

---

## Stop-gap declarations

- No new stop-gaps. This session *closes* TD-022.

---

## Verification & Testing Strategy

### 1. Unit Tests in `crates/photohelper-sidecar/src/lib.rs`
- **Unknown Field Preservation**: Create a test where an XMP sidecar has `<rdf:Description crs:ToneCurveName2012="Custom" crs:CameraProfile="Adobe Standard">`. Write `SidecarSettings`. Read the resulting file as a raw string and assert via `str::contains` (bypassing the parser) that fields are exactly preserved.
- **Child Element Re-injection & Sibling Preservation**: Create a test where an XMP sidecar has an existing `dc:subject`. Assert that the resulting XML has exactly one `dc:subject` block with the new bag, and subsequent elements are perfectly preserved. Include tests covering nested self-closing tags inside dropped blocks (e.g., `<rdf:li/>` inside `<dc:subject>`) to verify depth tracking doesn't desync, and ensure target `rdf:Description` being self-closing correctly expands.
- **Creation From Scratch**: Verify that writing to a non-existent sidecar directly emits a valid XMP document complete with `xpacket` instructions. Test the `ForceOverwrite` strategy bypassing the stream and successfully replacing a corrupted file.
- **Malformed XML & Temp File Cleanup**: Pass a corrupted XML file and assert that the writer returns `Error::XmlParse`, does NOT overwrite the target file, and the intermediate temporary file is verified deleted from disk.
- **Duplicate Attribute Guard**: Given an existing XML with `ph:NimaScore="3"`, updating the score to `5` must yield exactly one `ph:NimaScore="5"` attribute.
- **Attribute Deletion and Edge Boundaries**: Add a test verifying that `Update::Clear` completely removes an existing attribute from the output XML, and test edge cases like empty strings or zero values. Acknowledge that the new `Vec` based parsing will preserve order, so tests can assert the exact preserved string layout.
- **Multiple `rdf:Description` Tags**: Provide an XMP with two `rdf:Description` elements, asserting that injected attributes and children *only* appear in the first one, and the second is passed through pristine.
- **Namespace Injection Boundary**: Provide a pristine third-party XMP without `ph:` or `lr:` namespaces, write managed settings, and assert the output correctly injects the namespace declarations without duplication.
- **Valid XML Missing `rdf:Description`**: Verify that an existing valid XML missing `rdf:Description` strictly returns `Error::MissingRdfDescription`.
- **Atomic Persist Permission Failure**: Create a read-only target file, attempt update, and assert the operation yields an I/O error while successfully cleaning up the tempfile without modifying the target.

### 2. Verification command
- Run `just ci` to guarantee all existing tests plus new XMP round-trip tests compile, format, and pass cleanly.

---

## Synchronization Compliance

All references, paths, variable types, and exit codes defined or used in this plan strictly adhere to `docs/quality-assurance.md § State & Context Synchronization Discipline`.
