---
name: auto-implement
description: Run the session implementation autonomously from plan to finish. Executes the plan-review-loop, implements all planned features and test cases, tackles Technical Debt (TDs), runs verifications, and concludes with the implementation-review-loop. It defers questions to the user only as an absolute last resort, prioritizing progress. Use this after a plan is drafted to reach a PR-ready state.
argument-hint: "[session-NN]"
allowed-tools: Read Grep Glob Write Edit Agent Bash
---

Run the **auto-implement** skill to autonomously drive a session from a drafted plan all the way to a PR-ready state without human intervention.

## Autonomous Directives

*   **Zero-Interruption Policy**: Push forward as much as possible. If a decision is ambiguous, choose the most architecturally sound path, document the assumption, and keep building. Only pause for human input if the blocker is fatal and completely prevents further implementation in any other area of the codebase.
*   **Completeness**: You are responsible for the entire lifecycle of the session deliverables, tests, and technical debt.

## Steps

1.  **Harden the Plan**:
    *   Execute the `plan-review-loop` skill.
    *   Wait for the loop to complete and yield a perfectly clean, consolidated plan review.
2.  **Implementation Execution**:
    *   Systematically implement all deliverables, modules, and tests outlined in `docs/plans/session-NN.md`.
    *   Ensure all new features have complete test coverage and adhere to `CLAUDE.md`.
3.  **Technical Debt (TD) Resolution**:
    *   Tackle and implement any `TECH-DEBT.md` items that were scoped for this session.
4.  **Continuous Verification**:
    *   Run workspace verifications (e.g., `just ci`, `cargo test`, linting).
    *   Autonomously read the error logs, fix any failing tests or build errors, and repeat until the build is 100% green.
5.  **Harden the Implementation**:
    *   Execute the `implementation-review-loop` skill.
    *   Wait for the loop to completely resolve all architectural, logical, and style findings.
6.  **Readiness Declaration**:
    *   Once the build is green, the plan is complete, and the implementation review is clean, stop.
    *   Report to the user: "Session implementation is complete and PR-ready. All reviews and CI checks have passed. You may now run the `session-end` skill to ship."
