# ADR-0002 — LibRaw LGPL §6(a) static-link mechanics

> **Status**: DRAFT — Accepted pending legal review before the first GitHub
> Release tag is cut. The mechanism described here is committed to by every
> Release; the wording must be cleared with counsel before any binary ships
> publicly.
>
> **Date**: 2026-05-28
> **Authors**: Paulo Henrique Lerbach Rodrigues (Claude Code, session 02)
> **Supersedes**: none
> **Related**: `docs/discovery-notes.md § DN-001`,
> `docs/analysis/ANL-001-libraw-cr3-preflight.md`,
> `docs/plans/session-02.md § Deliverable 2`,
> `TECH-DEBT.md § TD-004` (CVE monitoring; complements this ADR).

## Context

photohelper links LibRaw statically. LibRaw ships under the dual license
LGPL-2.1-only OR CDDL-1.0; the project's `LICENSE.LGPL` file in the
vendored tarball is what we ship under. The LGPL-2.1 §6 grants permission
to distribute a "work that uses the Library" combined with the Library
itself, but requires the distributor to comply with one of two
sub-clauses:

* **§6(a)** — supply the complete machine-readable source of the Library
  alongside the combined binary, so a downstream user can re-link the
  binary against a modified Library.
* **§6(b)** — use a "suitable shared library mechanism" so the user can
  swap the Library at run-time without re-linking.

photohelper's `=0.22.1` LibRaw pin is statically linked into the
`photohelper` binary by `crates/photohelper-raw/build.rs`. §6(b) does
not apply (we do not ship LibRaw as a shared object). The compliance
mechanism is therefore §6(a): the LibRaw source must travel alongside
every binary distribution.

## Decision

Every photohelper GitHub Release whose binaries include the LibRaw FFI
MUST publish three artifacts in a single Release page:

1. **`photohelper-<target-triple>.tar.gz`** (or `.zip` on Windows) — the
   statically-linked photohelper binary plus its OS-appropriate manpage
   / launchd-plist / etc.

2. **`libraw-0.22.1.tar.gz`** — a byte-for-byte copy of the vendored
   tarball at `crates/photohelper-raw/vendor/libraw-0.22.1.tar.gz`. This
   is the §6(a) "complete machine-readable source of the Library" that
   the LGPL requires. A SHA-256 sidecar `libraw-0.22.1.tar.gz.sha256`
   ships next to the tarball so a downstream verifier can confirm the
   source we shipped matches what we built against.

3. **`README-LIBRAW-RELINKING.md`** — a short relinking instruction file
   adapted from the LibRaw upstream's `INSTALL`. Documents:
   - the exact `./configure` flags photohelper's `build.rs` used
     (`--disable-shared --enable-static --disable-jpeg --disable-lcms
     --disable-openmp --disable-examples`);
   - the LibRaw `=0.22.1` `git` tag the tarball corresponds to; and
   - the minimal toolchain needed (a C++ compiler, GNU/BSD make,
     `pkg-config`).

The Release notes template includes a one-paragraph LGPL summary linking
back to this ADR and to `libraw.org`.

### Ownership split with DN-001

DN-001 ("LibRaw LGPL static-link distribution mechanics") splits the work
across two sessions:

* **session 02 (this ADR)** owns the decision-doc and the build-system
  mechanism that produces `libraw-0.22.1.tar.gz` reproducibly.
* **release-engineering session** owns the GitHub Release workflow that
  uploads the tarball + SHA-256 + relinking README alongside every
  binary. That session also files for the legal-review sign-off that
  flips this ADR's status from DRAFT to ACCEPTED.

The build-system half is complete the moment `cargo build --release` on
a clean checkout produces a working `photohelper` binary AND
`crates/photohelper-raw/vendor/libraw-0.22.1.tar.gz` is the same byte
string that the release workflow will eventually upload. Both conditions
are met by the Deliverable 2 build-system commit; no additional plumbing
is required from session 02.

### Why §6(a), not §6(b)

Three reasons:

1. **Single-binary distribution is the user-visible value proposition**.
   The CLAUDE.md repo description names "single binary on Linux / macOS
   / Windows with no Python or Node runtime dependency for end users"
   as the explicit goal. A `.dylib` / `.so` / `.dll` of LibRaw alongside
   the binary defeats that.

2. **§6(b) wants "suitable shared-library mechanism"**. The shared
   library would need to be ABI-stable across patch versions, and the
   user would need to have it installed already (a Homebrew dependency,
   a Linux distro package, a Windows runtime download). Each of those
   is a UX failure mode.

3. **The §6(a) artifact is small**. The vendored tarball is 1.6 MB —
   trivial alongside a 50 MB-ish photohelper binary. Shipping it costs
   nothing and the relinking instructions are short.

The DN-001 record corrects an earlier plan-review confusion (PR1-T17)
that conflated §6(a) and §6(b); the literal LGPL-2.1 text was re-read
during R1 remediation and §6(a) is what governs our case.

## Status: pending legal review

This ADR is binding the moment the build-system mechanism lands (commit
SHA TBD in this session). However, the precise English wording of the
Release-notes LGPL summary, of `README-LIBRAW-RELINKING.md`, and the
Release-page disclaimer text MUST be reviewed by counsel before the
first GitHub Release tag is cut. The release-engineering session owns
that review.

Until that review completes, the ADR carries Status `"Accepted pending
legal review before first GitHub Release tag"` per the plan §Acceptance
criterion 5.

## Consequences

* **Positive** — every photohelper Release ships LGPL-compliant.
  Re-linkers can rebuild photohelper from source against a modified
  LibRaw. Audit-trail is reproducible from any prior Release.
* **Negative** — every Release upload adds ~1.6 MB of LibRaw tarball
  per platform variant (Linux × macOS × Windows = 4.8 MB extra). On a
  free-tier GitHub Release this is trivial.
* **Operational** — TD-004 ("LibRaw C-library CVE monitoring is manual")
  becomes a binding-trigger gate before every Release: the
  release-engineering session MUST confirm there are no open CVEs
  against the vendored `=0.22.1` before tagging. If an open CVE
  surfaces, photohelper either backports the fix via a vendored-source
  patch (and amends this ADR + the SHA-256 sidecar) or pins a newer
  LibRaw (and amends this ADR + the version pin everywhere).

## Alternatives considered

* **§6(b) shared-library mechanism**: rejected per § Why §6(a), not §6(b).
* **`libraw-sys` crate (third-party Rust crate vendoring LibRaw)**:
  rejected per plan-review R2-T14 — adding a third-party between us and
  LibRaw upstream complicates §6(a) source-distribution (we'd need to
  ship `libraw-sys`'s source too) and creates a per-version upgrade
  lag.
* **Dynamic-link against system LibRaw via `pkg-config`**: rejected — would
  shift LibRaw from "shipped with photohelper" to "Homebrew dependency",
  fragmenting the install story across distros and breaking single-binary
  distribution.
* **`cmake`-driven build**: rejected during the Deliverable 2 commit —
  LibRaw 0.22.1's tarball ships only autoconf scripts; the cmake build
  rules live in a separate `LibRaw/LibRaw-cmake` upstream repo. Vendoring
  a second repo would double the §6(a) tarball-shipping surface.
