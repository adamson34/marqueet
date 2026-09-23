# ADR-0010: `main` is only updated for releases

- **Status:** Accepted (amends ADR-0006)
- **Date:** 2026-09-23

## Context

ADR-0006 promoted each reviewed phase from `dev` to `main`. Phase 1 was
promoted that way. Before v1 there are no users tracking `main`, and
per-phase promotion PRs add churn without adding safety: every change already
lands on `dev` through a reviewed, CI-checked pull request.

## Decision

- All work keeps landing on `dev` through feature-branch PRs, one per piece of
  work (a phase is several PRs).
- `main` is not updated per phase. It receives one `dev` → `main` pull request
  for the v1 release (merge commit, so the branches share history), and later
  for each release.
- `main` protection from ADR-0006 is unchanged.

## Consequences

- `dev` is the branch to build and test from until v1; the README says so.
- The v1 promotion PR will be large; its description should summarize the
  changelog rather than list commits.
