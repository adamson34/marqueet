# ADR-0006: `dev` is default; feature branches and PRs; `main` is protected

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

The project is built in reviewed phases, with automated dependency updates
and CI that includes an ARM64 build.

## Decision

- `dev` is the default branch. All work happens on short-lived feature
  branches (`feat/`, `fix/`, `chore/`, `docs/`) with a pull request into `dev`.
- `main` holds approved phases and releases. A ruleset requires a pull request
  and passing CI (`fmt, clippy, test`, the ARM64 build, `cargo-deny`); no force
  pushes or deletion.
- `dev` blocks force pushes and deletion.
- Dependabot targets `dev`. Squash or merge commits; branches auto-delete
  after merge.

## Consequences

- Every change has a PR with CI evidence and a place for review.
- Promoting a phase is a single `dev` → `main` pull request.
