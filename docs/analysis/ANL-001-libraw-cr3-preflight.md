# ANL-001 — LibRaw CR3 pre-flight (EXIF extraction + CVE posture)

> **Session**: 02 (`libraw-cr3-decode`)
> **Deliverable**: 0 (pre-flight feasibility probe; gating gate for Deliverables
> 1-7)
> **Date**: 2026-05-28
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Status**: PROCEED. Pre-flight passes; chosen LibRaw pin is `=0.22.1`
> (escalated from plan's `=0.21.4` per § LibRaw version below).
> **Closes**: DN-018 (LibRaw vendored-tarball CVE-posture-as-of-pin audit owner)

## § Pre-flight context

`docs/plans/session-02.md § Deliverable 0` requires a one-shot probe BEFORE
the FFI work (Deliverables 1, 4) lands, answering two questions:

1. Does LibRaw actually extract the six EXIF fields photohelper needs
   (Make, Model, Orientation, CaptureTime, Width, Height) from the user's
   Canon R8 firmware revision, across the full 371-file corpus at
   `/Users/ph/Pictures/tests`?
2. Is the chosen LibRaw pin CVE-clean as of the pin date?

Either probe failing → ABORT, escalate to plan-review v4 (per the plan's
ABORT trigger and DN-018's binding trigger).

This artifact records the result of both probes and the version-pin
decision they forced.

## § LibRaw version

**Chosen pin: `=0.22.1`** (LibRaw 0.22.1 stable; tagged 2026-04-06 per
[`gh api repos/LibRaw/LibRaw/releases/tags/0.22.1`](https://github.com/LibRaw/LibRaw/releases/tag/0.22.1)).

**Plan default was `=0.21.4`** (2025-04-13). Per the plan's allowance —
"if libraw.org's current 0.21.x latest differs at pre-flight time, the
implementer picks the actual latest 0.21.x and amends this plan +
decision-doc 0002 with the actual version" — the implementer is empowered
to pick a different 0.21.x patch. Choosing 0.22.1 EXCEEDS that authority
(crosses the major-series boundary) and required user consultation per
the No-Acceptable-Trade-offs Policy (CLAUDE.md). User approved the
escalation on 2026-05-28; this section records the rationale.

### Why 0.22.1 over 0.21.4 / 0.21.5b

LibRaw's release timeline (via `gh api repos/LibRaw/LibRaw/releases`):

| Tag      | Date       | Notes                                              |
|----------|------------|----------------------------------------------------|
| 0.21.4   | 2025-04-13 | Plan default; OOB-read fixes in fuji/phase_one.    |
| 0.21.5   | 2025-12-24 | First 0.21.x release with that-version bug fixes.  |
| 0.21.5b  | 2025-12-25 | Last 0.21.x release; "0.21.5 (b)".                 |
| 0.22.0   | 2026-01-13 | Branch open.                                       |
| 0.22.1   | 2026-04-06 | Current stable; bottled in Homebrew.               |

`0.22.1`'s release notes ([`gh api releases/tags/0.22.1`](https://github.com/LibRaw/LibRaw/releases/tag/0.22.1))
enumerate six TALOS-2026 advisory fixes (security findings publicly
disclosed by Cisco TALOS' research team) and two CR3-parser-specific
hardenings directly relevant to photohelper's RAW pipeline:

- **CR3 parser: zero all buffers before fread** — closes an
  uninitialized-memory disclosure surface inside the CR3 parser.
- **CR3 parser: all file offsets are unsigned/64bit; check current offset
  against file size** — closes integer-overflow / file-truncation handling
  paths inside the CR3 parser.
- TALOS-2026-2364 — DNG float/deflated loader integer overflow.
- TALOS-2026-2363 — allocation-size integer overflow + EOF-in-read-loop check.
- TALOS-2026-2359 — X3F decoder allocation limit
  (`LIBRAW_X3F_ALLOC_LIMIT_MB`).
- TALOS-2026-2358 / -2331 / -2330 — additional hardening (release notes
  do not disambiguate further).

None of these fixes appear in the 0.21.x release notes for 0.21.5 /
0.21.5b. 0.21.5b shipped on 2025-12-25; the TALOS-2026-* disclosures
landed in 2026 (per the "2026" numeric prefix on the advisory IDs); the
0.22.1 release on 2026-04-06 is the version where the fixes land. The
0.21.x branch is effectively end-of-life — upstream has not signaled
backports.

Two of the eight fixes target the CR3 parser directly. photohelper's
v0.1 scope is "Canon CR3 ingest" — these are the fix-class we actually
exercise. Pinning 0.21.x would ship a binary that processes attacker-
controllable CR3 inputs through a CR3 parser missing publicly-disclosed
hardenings. That fails the plan's spirit even where it passes the plan's
literal CVE-feed grep.

### Implication for the plan + ADR-0002

The plan's Deliverable 0 §LibRaw version, Deliverable 2 §Build system,
Acceptance criterion 7, and Plan revisions log all reference `=0.21.4`.
This session's `chore(libraw): pre-flight EXIF + CVE-posture audit
(Deliverable 0)` commit amends the plan in-place (`docs/plans/session-02.md`
v3.2) AND notes that ADR-0002 (still to be authored in Deliverable 2)
records `=0.22.1` not `=0.21.4`. The vendored tarball path becomes
`crates/photohelper-raw/vendor/libraw-0.22.1.tar.gz` and the SHA-256
sidecar `crates/photohelper-raw/vendor/libraw-0.22.1.tar.gz.sha256`.

## § CVE-posture-as-of-pin

**Result: CLEAN (versus MITRE NVD feed 2026-05-28; versus LibRaw GitHub
Security Advisories 2026-05-28).**

### MITRE NVD CVE feed

Query: [`services.nvd.nist.gov/rest/json/cves/2.0?keywordSearch=libraw`](https://services.nvd.nist.gov/rest/json/cves/2.0?keywordSearch=libraw)
(cached at `.scratch/nvd-libraw.json` during pre-flight; NOT committed —
the URL is the canonical source).

- Total historical LibRaw CVEs in NVD: 68 (oldest CVE-2013-2126, fixed
  in 0.15.2).
- LibRaw CVEs published since `pubStartDate=2023-01-01`: **0**.
- Open LibRaw CVEs affecting `=0.22.1`: **0**.
- Open LibRaw CVEs affecting `=0.21.4` (for comparison): **0**.

The TALOS-2026-* advisories named in 0.22.1's release notes do NOT yet
have MITRE CVE IDs assigned (TALOS findings often precede CVE assignment
by months; the 0.21.5b release on 2025-12-25 confirms the disclosure
window is recent enough that NVD has not yet caught up). Per the plan's
literal CVE-feed grep, both `=0.21.4` and `=0.22.1` pass the ABORT
trigger. The 0.22.1 pin is preferred on the OPERATIONAL grounds detailed
in § LibRaw version above — TALOS findings carry the same risk class as
CVEs even when not yet CVE-numbered.

### LibRaw GitHub Security Advisories

Query: [`github.com/LibRaw/LibRaw/security/advisories`](https://github.com/LibRaw/LibRaw/security/advisories)

- Published advisories: **0** (page explicitly states "There aren't any
  published security advisories"; the LibRaw project tracks security via
  release notes + TALOS coordination, not via GitHub's advisory UI).

### Ongoing monitoring

TD-004 (`LibRaw C-library CVE monitoring is manual; cargo audit does NOT
cover it`) is the long-term remediation. Per TD-004's binding trigger:
fires on first `crates/photohelper-raw` touch after 2026-08-01 OR any
LibRaw CVE disclosure OR before the first GitHub Release tag. Recommended
path: wire `osv-scanner` against `.osv-scanner.toml` declaring the
vendored LibRaw version, integrated into `just ci` after `cargo audit`.
Not in session 02 scope.

## § EXIF extraction

**Result: 370 / 370 CR3s extracted all six required fields. Pass-rate:
100% (well above the 95% ABORT threshold).**

### Method

Command: `/opt/homebrew/opt/libraw/bin/raw-identify -v <file>` (Homebrew
LibRaw 0.22.1 stable; same upstream we plan to vendor — sanctioned
because the CR3 parser code path is identical between the Homebrew
binary and the vendored tarball we will compile against in Deliverable
2). The probe script lives at `.scratch/libraw-preflight.sh` (NOT
committed — `.scratch/` is gitignored as scratch state) and ran in 6.6s
wall-clock for the full corpus.

### Counts

| Field             | Pass / Total | Notes                                  |
|-------------------|--------------|----------------------------------------|
| Camera line       | 370 / 370    | `Camera: <Make> <Model> ID: 0x...`     |
| `Canon EOS R8`    | 370 / 370    | Every fixture is the same R8 body.     |
| Image size        | 370 / 370    | `Image size: 6022 x 4024` (post-rot).  |
| Timestamp         | 370 / 370    | `Timestamp: <ctime>` UTC.              |
| Image flip        | 370 / 370    | `Image flip: <0..7>`.                  |

The fixture directory `/Users/ph/Pictures/tests/` contains 371 entries:
370 `*.CR3` files + 1 `.photohelper/` catalog directory (left over from
the user's earlier `photohelper ingest` smoke run that produced DN-011).
The photohelper walker filters `.photohelper/` via
`filter_entry(|e| e.file_name() != ".photohelper")` so it counts as
walked: 371, skipped (non-RAW): 1, ingest-attempted: 370 — matching the
plan's Acceptance 2b expected summary.

### Field mapping (LibRaw → photohelper)

| photohelper field         | LibRaw API call                            | raw-identify -v output            |
|---------------------------|--------------------------------------------|-----------------------------------|
| `make`                    | `libraw_get_iparams().make`                | `Camera: <make> <model>` (1st tok)|
| `model`                   | `libraw_get_iparams().model`               | `Camera: <make> <model>` (rest)   |
| `width`                   | `libraw_get_iwidth()`                      | `Image size: <w> x <h>` (1st)     |
| `height`                  | `libraw_get_iheight()`                     | `Image size: <w> x <h>` (2nd)     |
| `capture_time_unix_secs`  | `libraw_get_iparams().timestamp`           | `Timestamp: <ctime>` (parsed)     |
| `orientation`             | `imgdata.sizes.flip`                       | `Image flip: <0..7>`              |

LibRaw's `flip` value differs from EXIF's `Orientation` tag (LibRaw
returns post-rotation; EXIF tag is pre-rotation). The Deliverable 1c
`RawExif` constructor MUST translate `flip ∈ {0,3,5,6}` to the matching
`ExifOrientation` variant; out-of-range → `RawExifCause::ExifMalformed { field: "orientation", raw_value }`.
Sample fixture `_MG_9625.CR3` returned `Image flip: 0` (no rotation;
landscape orientation; corresponds to `ExifOrientation::Normal`).

### Spot-check sample

```
$ /opt/homebrew/opt/libraw/bin/raw-identify -v /Users/ph/Pictures/tests/_MG_9625.CR3
Filename: /Users/ph/Pictures/tests/_MG_9625.CR3
Timestamp: Sat Mar  7 06:08:34 2026
Camera: Canon EOS R8 ID: 0x80000487
Normalized Make/Model: =Canon/EOS R8= CamMaker ID: 8
...
Raw inset, width x height: 6000 x 4000 left: 168 top: 108
Image size:  6022 x 4024
Image flip: 0
```

All six required fields present. The 6022 × 4024 dimensions match the
Canon R8 sensor's documented active-pixel area (24Mpix nominal, with
LibRaw reporting the visible-area post-crop dimensions).

### Failures

None. `.scratch/libraw-preflight-failures.log` is empty after the run.

## § Decision

**PROCEED with Deliverable 1 (FFI module), pinned to LibRaw `=0.22.1`.**

ABORT triggers (per plan §Deliverable 0):
- `> 5%` EXIF-extraction failure: **NOT FIRED** (0% failure).
- Any open MITRE CVE on chosen version: **NOT FIRED** (NVD shows 0 since
  2023; GHSA shows 0).

Plan amendments folded into the same Deliverable-0 commit:
- `docs/plans/session-02.md` v3.1 → v3.2: every `0.21.4` reference becomes
  `0.22.1` (Deliverable 0, Deliverable 2, Acceptance criterion 7, Plan
  revisions log).
- `docs/discovery-notes.md § DN-018`: status flipped to closed (Deliverable
  0 owner satisfied).

ADR-0002 (LibRaw LGPL static-link mechanics) is still to be authored in
Deliverable 2; it MUST cite `=0.22.1` not `=0.21.4` as the vendored
version.

The next commits (Deliverable 1 FFI module → Deliverable 2 build-system +
ADR-0002 → ...) may now proceed.
