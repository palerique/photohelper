# Decision 0004 — GitHub Actions version pinning

**Date**: 2026-05-29 (session 06, TD-001)
**Status**: Active

## Decision

Pin all `uses:` directives in `.github/workflows/ci.yml` to commit SHAs instead of floating
`@vN` tags, per TD-001 binding trigger (pre-release supply-chain hardening).

## Rationale

A floating tag (`actions/checkout@v4`) can be moved to a different commit by the action
author. A compromised action could exfiltrate secrets or inject malicious code into the
build without any change to this repository. Pinning to a commit SHA provides a
cryptographic guarantee that the exact code version is used.

## Pinned versions (2026-05-29)

| Action | Version | Commit SHA |
|---|---|---|
| `actions/checkout` | v4.3.1 | `34e114876b0b11c390a56381ad16ebd13914f8d5` |
| `dtolnay/rust-toolchain` | HEAD (2026-05-29) | `3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9` |
| `Swatinem/rust-cache` | v2.9.1 | `c19371144df3bb44fab255c43d04cbc2ab54d1c4` |

## Upgrade cadence

Review annually or when a CVE is disclosed for any of these actions.
To upgrade: look up the new tag's commit SHA via GitHub API, update both
`.github/workflows/ci.yml` and this file in the same commit.

```bash
# Example: find SHA for a new tag
curl -s "https://api.github.com/repos/actions/checkout/git/refs/tags/v4.X.Y" \
  | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['object']['sha'])"
```
