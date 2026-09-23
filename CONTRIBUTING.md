# Contributing to Marqueet

Thanks for helping! A few ground rules keep the project small and easy to run
on cheap hardware.

## Branches

- **`main`** holds releases only. Until v1 everything lives on `dev`
  ([ADR-0010](docs/adr/0010-main-is-for-releases.md)).
- **`dev`** is where work lands. **Open pull requests against `dev`.**
- Use short-lived feature branches, e.g. `feat/espn-provider`,
  `fix/crawl-flicker`, `docs/readme-hardware`.

## Before opening a PR

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check        # cargo install cargo-deny
```

CI runs the same checks (plus an ARM64 build) and must pass before merging.

For visual changes, attach before/after images:

```sh
cargo run --release -p marqueet-display -- --mock --screenshot after.png
```

If you edit the logo (`crates/core/assets/*.txt`), regenerate the SVGs with
`MARQUEET_BLESS=1 cargo test -p marqueet-core logo`. Update `CHANGELOG.md` and
the checklists in `docs/ROADMAP.md` in the same PR. Architectural decisions
get an ADR in `docs/adr/`.

## Project principles

- **Rust everywhere.** No npm, no JavaScript build toolchain, no third-party
  JS libraries. The admin page is server-rendered HTML/CSS with, at most, a
  small hand-written script.
- **Few, well-maintained dependencies.** Every new crate is a long-term
  maintenance and security cost; explain why it's needed in the PR.
  `cargo deny` blocks crates with known advisories.
- **Must run on a Raspberry Pi 4.** Rendering stays within WebGL2 / GLES 3.0
  limits; avoid per-frame CPU work that scales with content.
- **Pure core.** Parsing, normalization, formatting and event detection live in
  `marqueet-core` with no I/O, so they can be unit tested with fixtures.

## Commit messages

Short imperative subject (`Add ESPN scoreboard normalizer`), optional body
explaining why.
