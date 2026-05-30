# ADR-0003 — Lightroom Compatibility XMP Architecture

> **Status**: ACCEPTED
> **Date**: 2026-05-30
> **Authors**: Paulo Henrique Lerbach Rodrigues ( Claude Code, session 07 )
> **Supersedes**: none
> **Related**: `docs/discovery-notes.md § DN-029`,
> `docs/plans/session-07.md § D5`,
> `TECH-DEBT.md § TD-022` (XMP hand-rolled quick-xml manual template instead of SDK).

## Context

To resolve **DN-029** (Lightroom Classic custom namespace incompatibility), `photohelper` must map computed AI scores (NIMA culling scores) and duplicate cluster IDs directly into standard Lightroom-compatible fields (star ratings `xmp:Rating`, color labels `xmp:Label`, flat keywords `dc:subject`, and hierarchical keywords `lr:hierarchicalSubject`) inside `.xmp` sidecar files.

Doing so requires a robust XML manipulation layer that satisfies three non-negotiable constraints:
1. **No Catastrophic Metadata Loss (Non-Clobbering)**: If a user has already made edits (such as custom exposure, local adjustments, crop points, camera profiles, or radial gradients) in Lightroom Classic, those unmodeled properties must be preserved exactly as-is. Our writer must perform a non-destructive deep merge rather than overwriting the sidecar with a bare template.
2. **Zero Compilation-Phase C/C++ FFI Dependencies**: The repository enforces a strict architectural mandate to build portable, single-binary executables with zero system-level C/C++ runtimes or compilation-phase FFI pkg-config requirements (maintaining MSRV 1.88 and single-binary portability).
3. **High-Performance and Concurrency-Safe**: The CLI `develop` subcommand executes sidecar writes in parallel across thousands of RAW images using Rayon. The I/O layer must be extremely fast, memory-bounded, and free of concurrency race hazards.

## Evaluated Alternatives

We evaluated three potential strategies for XMP parsing, merging, and writing:

### 1. `xmp-writer` (Pure-Rust crate)
* **Pros**: Pure Rust, lightweight, and very clean API for creating compliant XMP sidecars.
* **Cons**: Strictly **write-only / creation-only**. It lacks any parsing, streaming, or querying capabilities. Utilizing `xmp-writer` would require us to either completely overwrite existing sidecars (violating the non-clobbering mandate) or write a separate custom XML parser to extract existing fields first. Overwriting would destroy pre-existing user adjustments, causing catastrophic data loss.
* **Verdict**: Rejected.

### 2. `rexiv2` (GObject/Exiv2 wrapper crate)
* **Pros**: Highly capable, wraps the industry-standard C++ `Exiv2` library, and natively supports non-destructive XMP/EXIF read-write-merge round-tripping.
* **Cons**: Introduces a heavy, brittle **C/C++ FFI build dependency** on `libexiv2` and pkg-config. This violates our strict architectural mandate for a portable, single-binary cargo compile with zero system-level C++ toolchain constraints.
* **Verdict**: Rejected.

### 3. Streaming Event-Driven Parser via `quick-xml` (Our Choice)
* **Pros**: Pure Rust, extremely fast, memory-bounded, and already integrated into the workspace. It compiles anywhere and has no FFI overhead.
* **Cons**: Requires manual XML structure reconstruction and serialization logic to perform deep merging.
* **Verdict**: **Accepted**. By leveraging `quick-xml`'s high-performance streaming parser (`Reader`) and writer (`Writer`), we can parse existing XMP files event-by-event, extract active fields (ratings, labels, keywords, and Camera Raw sliders), perform an in-memory deep merge, and write them back atomically.

## Decision

Implement the XMP sidecar reading, merging, and writing architecture in `crates/photohelper-sidecar` using a pure-Rust event-driven model powered by `quick-xml`.

### Key Architectural Rules

1. **Prefix-Agnostic Tag Matching**: Standardize on local-name matching (`Rating`, `Label`, `subject`, `hierarchicalSubject`) with prefix-agnostic capabilities (supporting both `xmp:Rating`, `xap:Rating`, or custom namespace prefixes) to guarantee we never silently miss standard fields written by third-party editors.
2. **Flat Parsing Loop with Stack Safety**: Use a single flat outer parsing loop over `reader.read_event()` with a qualified tag stack (`Vec<String>` representing open tags) to track the exact element path (e.g., `["dc:subject", "rdf:Bag", "rdf:li"]`). This eliminates nested sub-loops that can get stuck in infinite CPU-spinning cycles on premature EOF or corrupted closing tags. Limit tag stack depth to **64** to prevent OOM panics on malformed files.
3. **Decimal Integer Parsing Resilience**: To handle decimal-formatted integer attributes written by some industry tools (e.g., `xmp:Rating="3.0"`), integer parsing helpers must parse inputs as `f64` first, check if finite and within bounds, round them, and then cast to integer.
4. **Non-Clobbering Append-Only Merging**: During merge, preserve all existing standard fields (ratings, labels, keywords) if incoming update fields are `None`. Keep all unmodeled namespaces (`crs:`, custom Adobe namespaces, camera profiles, etc.) exactly as-is by streaming unmodeled events intact.
5. **Lightroom Classic Color Label Clearing Mechanics**: In Lightroom Classic, omitting the `xmp:Label` attribute entirely does NOT clear an existing color label in its catalog. To explicitly clear a color label, `photohelper` must write `xmp:Label=""` (empty string attribute). Represent this explicitly in memory as `Some(String::new())` rather than flattening empty labels to `None`.
6. **Thread-Unique Temporary Suffixes for Concurrency Safety**: To prevent Rayon write collisions when processing multiple files concurrently (such as duplicate RAW files, virtual copies, or images in the same directory), append a thread-unique identifier to temporary files (e.g., `.phdev.<thread_id>.tmp`) before atomically renaming them.

## Consequences

* **Portability**: Remains 100% portable and compile-safe with zero C/C++ FFI dependencies.
* **Performance**: Maintains high-throughput parallel processing with Rayon, with minimal memory footprint and zero context-switch overhead from `SystemTime::now()` system calls.
* **Safety**: Prevents silent metadata loss by ensuring non-destructive blending of existing user edits.
* **Compatibility**: Native Lightroom Classic integration works out of the box without requiring a separate Lua/C++ SDK plugin.
