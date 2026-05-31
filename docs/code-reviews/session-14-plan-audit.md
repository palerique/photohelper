# Structural & Correctness Audit Report

The elite system architecture and algorithmic correctness audit of `docs/plans/session-14.md` has surfaced 4 significant flaws.

### 1. Cross-Device Link Panic on `NamedTempFile` (`EXDEV`)
- **Severity**: **CRITICAL**
- **File:Line**: `docs/plans/session-14.md:27`
- **Finding**: The plan specifies using `tempfile::NamedTempFile` to ensure automatic temporary file cleanup. By default, `NamedTempFile::new()` creates the temporary file in the OS temp directory (e.g., `/tmp`). When `persist()` is called to overwrite the target sidecar, it uses a hard link or atomic rename. Because photo libraries are frequently stored on external drives or NAS volumes (different mount points than the boot drive's `/tmp`), this will routinely fail with a Cross-Device Link error (`std::io::ErrorKind::CrossesDevices` / `EXDEV`), entirely breaking the sidecar writer for external drives.
- **Remediation**: Explicitly specify using `tempfile::NamedTempFile::new_in(target_file.parent().unwrap())`. This guarantees the temporary file is created on the exact same filesystem mount as the target XMP, allowing `persist()` to succeed atomically.

### 2. Missing Structural Anchor for `rdf:Description` Re-injection
- **Severity**: **HIGH**
- **File:Line**: `docs/plans/session-14.md:18` (and Line 22)
- **Finding**: The plan mandates that a missing `rdf:Description` must be "placed correctly inside `x:xmpmeta/rdf:RDF`, NOT appended at EOF." However, the proposed `WriterState` enum (`SeekingDescription`) lacks the context to achieve this. If the parser is in `SeekingDescription`, it will continuously pass through events until EOF. It has no mechanism to intercept the exact moment it is leaving the `rdf:RDF` or `x:xmpmeta` scopes, meaning it will either miss the injection entirely or be forced to append it out-of-bounds after EOF, breaking the XML schema.
- **Remediation**: The state machine loop must explicitly hook `Event::End` for `b"rdf:RDF"` (or `b"x:xmpmeta"`). If `State == SeekingDescription` when encountering this closing tag, it must immediately inject the new `<rdf:Description>` block *before* writing the closing event, and then transition to `InjectionComplete`.

### 3. Data Leak / State Corruption on Multiple `rdf:Description` Tags
- **Severity**: **HIGH**
- **File:Line**: `docs/plans/session-14.md:62`
- **Finding**: The plan dictates that if multiple `rdf:Description` tags exist, we inject into the first one and the second is "passed through pristine". This is an XMP correctness vulnerability. If an existing sidecar has managed fields (like an old rating or an old `<dc:subject>`) split across a second `rdf:Description` block, passing it through pristine will retain the *old* data. When the file is loaded by a reader, the XMP parser will see duplicate properties and either crash, merge them incorrectly, or favor the stale data over our managed data.
- **Remediation**: Redefine the final state. `InjectionComplete` must still intercept and strip/clear managed attributes and drop managed child elements (e.g., `dc:subject`) across *all* subsequent `rdf:Description` tags. It should only skip *re-injecting* the new values, ensuring all stale data is completely purged from the entire document.

### 4. Ambiguous Child Element Re-injection Hook
- **Severity**: **MEDIUM**
- **File:Line**: `docs/plans/session-14.md:58`
- **Finding**: The plan mentions dropping child elements (like `dc:subject`) and asserts they must be re-injected with the new bag, but it does not specify *when* in the event stream they are written. If they are injected at the exact moment the old element is dropped, they will fail to write entirely if the old element never existed in the original file.
- **Remediation**: Explicitly mandate that all new managed child elements (like `dc:subject` tags) must be injected exactly when intercepting the `Event::End` for the target `rdf:Description` tag, right before closing it. This ensures they are always written regardless of whether they existed previously.
