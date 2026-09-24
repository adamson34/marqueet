# Architecture Decision Records

Short records of decisions that shape Marqueet, so the reasons survive after
the conversation that produced them. Add a new file for each significant
decision; don't rewrite old ones. If a decision changes, add a new ADR that
supersedes it and mark the old one's status.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-rust-everywhere-no-js-toolchain.md) | Rust everywhere; no npm or JavaScript toolchain | Accepted |
| [0002](0002-native-wgpu-display.md) | The display is a native wgpu app, not a browser kiosk | Accepted |
| [0003](0003-generic-segments-and-alerts.md) | Sources produce generic ticker segments and alerts | Accepted |
| [0004](0004-led-rendering-pipeline.md) | LED rendering: CPU rasterizes once, GPU scrolls and glows | Accepted |
| [0005](0005-dot-grid-brand-source-of-truth.md) | The logo is a dot grid that generates every asset | Accepted |
| [0006](0006-branching-and-review-flow.md) | `dev` is default; feature branches and PRs; `main` is protected | Amended by 0010 |
| [0007](0007-led-ticker-flat-widgets.md) | LED for the ticker; flat UI cards for widgets and takeovers | Amended by 0012 |
| [0008](0008-name-marqueet.md) | The project is named Marqueet | Accepted |
| [0009](0009-espn-provider.md) | ESPN provider: honest client, lenient parsing, ring TLS | Accepted |
| [0010](0010-main-is-for-releases.md) | `main` is only updated for releases (first: v1) | Accepted |
| [0011](0011-packaging-snap.md) | Ship as a snap | Accepted |
| [0012](0012-display-themes.md) | Display themes: a style plus a palette | Accepted |
| [0013](0013-bring-your-own-team-art.md) | People bring their own team colors and logos | Accepted |

Template: **Status**, **Date**, **Context**, **Decision**, **Consequences**.
