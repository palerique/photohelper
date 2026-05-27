# Session-handoff format

> Canonical format for the final session-end response (after the PR merges or
> session work is otherwise complete). Stack-agnostic.
>
> This file is the single source of truth for the session-end shape. Any tool
> (a Claude Code session, a CI script, a release-notes generator) that produces
> session handoffs MUST emit the two artefacts below as the final blocks of the
> handoff message. Verbatim consistency is the contract — downstream tooling
> parses on these exact shapes.

## Mandatory artefacts (in order)

Every session-end response ends with **exactly these two blocks**, in this
order:

1. **`Session NN final summary` table** — the at-a-glance metrics block.
2. **Next-session bash bootstrap** — a literal four-command snippet in a `bash`
   code fence.

No substitutions, no reformatting, no skipping either block.

## Block 1 — Session NN final summary

Heading line: `## Session NN final summary` (`NN` zero-padded for `01..09`;
literal `06.5` for sub-sessions per `CLAUDE.md § Branch-name convention`).

Table rows in this order; value column right-aligned:

| Metric | Value |
|--------|------:|
| **Merge commit** | `<git sha>` (or "pending merge" if not yet merged when rendered) |
| **PR** | `https://github.com/<org>/<repo>/pull/<N>` |
| **Commits on branch** | integer from `git log --oneline main..HEAD \| wc -l` (before the merge commit) |
| **Plan-review agent fires** | `N (R1×8 + R2×N)` — N is 8 for a full sweep, fewer for a regression-watch round |
| **Session-end agent fires** | `N (R1×8 + R2×N)` — same shape |
| **Total subagent fires** | `N / <ceiling> (M unused)` — per the cadence chosen in `docs/quality-assurance.md` |
| **Plan iterations** | `v1 → v2 → v3` with brief delta notes |
| **TDs touched** | `M ledger rows: X closures + Y close-as-merged + Z reschedules + W ledger-moves` |
| **CI** | `N/N jobs green` — N is the count of jobs in `.github/workflows/ci.yml` |
| **New tests** | `N (all pass)` — net-new test functions added this session |
| **Public API break** | `yes — describe shape` OR `no` |

Followed by **two trailing sentences**:

- **Closes / Reschedules**: explicit TD IDs grouped by action verb. Example:
  "**Closes**: TD-020, TD-025 (2). **Reschedules**: TD-016 → session 7 (with
  forcing-function row in `SESSION-STATE.md`)."
- **Next session**: pointer to the upcoming scope + branch start condition.
  Example: "**Next session (session 7)**: optional sinks + production fetcher.
  Branch starts clean from `main`."

### Fill-in skeleton

```markdown
## Session NN final summary

| Metric | Value |
|--------|------:|
| **Merge commit** | `XXXXXXX` |
| **PR** | https://github.com/<org>/<repo>/pull/N |
| **Commits on branch** | N |
| **Plan-review agent fires** | N (R1×8 + R2×N) |
| **Session-end agent fires** | N (R1×8 + R2×N) |
| **Total subagent fires** | N / <ceiling> (M unused) |
| **Plan iterations** | v1 → v2 → v3 with brief deltas |
| **TDs touched** | M ledger rows: X closures + Y reschedules + Z ledger-moves |
| **CI** | N/N jobs green |
| **New tests** | N (all pass) |
| **Public API break** | yes/no — describe shape if yes |

**Closes**: TD-..., TD-... (N). **Reschedules**: TD-... → session N+1 (forcing-function row N).

**Next session (session N+1)**: <one-sentence scope from SESSION-STATE.md>. Branch starts clean from `main`.
```

## Block 2 — Next-session bash bootstrap

A `bash` code fence with exactly the session bootstrap from
`CLAUDE.md § Mandatory session protocol` step 1, next branch filled in:

```bash
git switch main && git pull --ff-only origin main && git switch -c session-NN/<kebab-slug> && just session-start
```

**Branch-name derivation:**

1. `NN` = next session number, zero-padded for 1–9; literal `10`, `11`, …
   thereafter; literal `06.5` for sub-sessions.
2. `<kebab-slug>` = the primary component from `SESSION-STATE.md § Component
   progress`, lowercased + hyphen-separated. Drop articles, prepositions, and
   decorative qualifiers.
3. If multiple slugs are plausible, name 1–2 alternatives in one sentence after
   the fence.

## Authoring notes for tools

- **Always emit both blocks, always last.** Other content goes above them.
- **Use the heading text verbatim** (`## Session NN final summary`).
- **Use the row labels verbatim** (the bold first-column cells); if a metric is
  N/A, write `n/a` rather than dropping the row.
- **Right-align the value column.**
- **Use a `bash` code fence** for the bootstrap.

## Why this format exists

- Verbatim consistency makes the two blocks grep-targetable across sessions.
- The bash phrase eliminates re-derivation friction — paste-and-go.
- The table surfaces what's otherwise lost to commit history: agent-fire
  counts, plan iterations, TD-touched breakdown.

## Cross-references

- `CLAUDE.md § Mandatory session protocol` — the lifecycle this format closes.
- `CLAUDE.md § Where things live` — registers this file in the artefact table.
- `docs/quality-assurance.md § Session-end protocol` — the per-step discipline
  this format is the final output of.
- `SESSION-STATE.md § Component progress` — source for the next-session slug.
