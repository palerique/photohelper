# photohelper — Tech-Debt Ledger

> Known shortcuts taken for velocity, each with a remediation plan and a
> **binding trigger**. This ledger is the canonical view of "where the codebase
> trades off quality vs. velocity right now."
>
> Policy: see `CLAUDE.md § No Acceptable Trade-offs Policy`. A stop-gap without
> a TD entry here is a process violation; a deferral without a plan is a
> CRITICAL finding on its own (`docs/quality-assurance.md § Findings triage`).

## Entry format

Each TD has a stable ID (`TD-NNN`) and these fields:

```markdown
### TD-NNN — <descriptive title>

- **Status**: Open | Closed (YYYY-MM-DD, session N; reason)
- **Opened**: YYYY-MM-DD (session N)
- **Stop-gap location**: <file:line> @ <commit-sha>
- **Fundamental fix**: <concrete implementation outline — not "investigate">
- **Binding trigger**: <rev-list-anchored | by YYYY-MM-DD | event-driven>
- **Scope estimate**: <~LoC> / <risk: low|med|high>
- **Consequence of inaction**: <if unaddressed, X happens>
- **Related**: <links to code-review artifacts, discovery-notes, ADRs>
```

---

## Open

### TD-001 — GitHub Actions action versions use `@vN` floating tags, not pinned SHAs

- **Status**: Open
- **Opened**: 2026-05-27 (session 0)
- **Stop-gap location**: `.github/workflows/ci.yml` (all `uses:` lines tagged `<<pin to SHA>>`) @ bootstrap commit
- **Fundamental fix**: replace every `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2` with the corresponding commit SHA from the action's repo; commit a `docs/decisions/0001-action-version-pinning.md` recording the SHAs chosen and the upgrade cadence. Add a periodic refresh task (Dependabot or scheduled session).
- **Binding trigger**: before the first PR from an external contributor merges, OR before the first GitHub Release tag is cut — whichever comes first.
- **Scope estimate**: ~20 LoC across `.github/workflows/ci.yml` + one new decision doc / low risk
- **Consequence of inaction**: a compromised upstream action could exfiltrate secrets or inject code into the build; the `<<pin to SHA>>` comments are visible reminders but not enforced.
- **Related**: `docs/discovery-notes.md` (none yet — this is a self-contained debt)

---

### TD-002 — `rusqlite` pinned at 0.32 instead of plan-v5 target 0.40 (CVE exposure)

- **Status**: Open
- **Opened**: 2026-05-28 (session 1)
- **Stop-gap location**: `Cargo.toml` `[workspace.dependencies]` `rusqlite = { version = "0.32", features = ["bundled"] }` @ commit `310f753` (initial implementation)
- **Fundamental fix**: bump to `rusqlite = "0.40"` (or whatever the latest version is at remediation time); run `cargo update -p rusqlite`; verify `just ci` stays green (rusqlite 0.40 is API-compatible for `Connection::open` / `execute` / `query_row` / `Transaction` / `params!` — the operations photohelper uses); confirm `cargo audit` does not flag the newer bundled SQLite version.
- **Binding trigger**: bump by **2026-08-01** OR before session 02 introduces new catalog schema columns (whichever first). Session 02 will modify `Catalog::upsert` paths anyway — bundling the dep bump into that change minimizes churn.
- **Scope estimate**: ~5 LoC (Cargo.toml + Cargo.lock auto-update + possibly a few rusqlite API-rename touchups if 0.32→0.40 deprecates anything we use) / low risk
- **Consequence of inaction**: any SQLite CVE released after rusqlite 0.32's bundled-amalgamation cutoff (mid-2024) will fail `cargo audit --deny warnings` → fail CI → emergency bump under time pressure. Sitting on a 14-month-old SQLite bundle is exactly the silent-failure-via-stale-dep pattern `cargo audit` exists to surface.
- **Related**: `docs/discovery-notes.md` DN-007 (cross-reference); `docs/code-reviews/session-01-round1.md § T5` (the finding that surfaced this).

---

## Closed

_(none yet)_
