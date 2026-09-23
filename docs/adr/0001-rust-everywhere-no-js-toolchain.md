# ADR-0001: Rust everywhere; no npm or JavaScript toolchain

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

The original brief proposed Node.js/TypeScript with Vite for the display and
admin page. Marqueet runs unattended on people's home networks for years. An
npm dependency tree brings hundreds of transitive packages, frequent
vulnerability advisories, and a build toolchain that has to be kept working.

## Decision

- Every component is written in Rust: server, display, providers, plugins.
- No npm, `package.json`, bundler or `node_modules` anywhere in the repo.
- The admin page is server-rendered HTML/CSS (askama). If interactivity needs
  script, it's a small hand-written file in the repo; no third-party JS
  libraries.
- Rust dependencies are kept few and well-maintained, audited in CI with
  `cargo-deny` (RustSec advisories, licenses, sources) and updated by Dependabot.

## Consequences

- One toolchain (`cargo`), static binaries, easy cross-compiles for ARM64.
- UI work (widgets, admin) takes more effort than it would with a web
  framework.
- The contributor pool is smaller than for a TypeScript project.
