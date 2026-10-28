# ADR-0017: Assistants run the sign through API keys, a tool API and MCP

- **Status:** Accepted
- **Date:** 2026-10-28

## Context

People want to say "make my team's goals look like this" and have Claude do
it, rather than learn the admin page. The admin page's actions were only
reachable as HTML forms behind a session cookie; scripts had the feed API,
which only puts content on the sign. Assistants like Claude Code connect to
tools through MCP (the Model Context Protocol), which they can reach over
HTTP with a header for credentials.

## Decision

- **API keys.** The admin page's **Assistants** section makes named keys
  (`mqk_` + 256 random bits), shows each once, keeps only its SHA-256 (like
  feed tokens), and revokes them. At most 8. `Authorization: Bearer <key>`
  gives the same access as a logged-in session to every admin route and
  JSON API. Keys outlive password changes (they are revoked on the page)
  but not a password reset from the SD card, which removes them all. They
  can't be used before first-boot setup.
- **One tool table** (`crates/server/src/tools.rs`): each admin-page action
  as a named tool with a JSON Schema for its arguments and a description
  written for an assistant. Making keys and changing the password are not
  tools: a key can't mint keys or lock the owner out.
- **Two ways in, same tools:** `GET /api/tools` and
  `POST /api/tools/<name>` for scripts, and `POST /mcp` for assistants.
- **MCP by hand.** `/mcp` implements the Streamable HTTP transport's
  stateless subset on `serde_json`: `initialize`, `ping`, `tools/list`,
  `tools/call`, notifications answered with `202`, no sessions and no
  server-sent events. No MCP SDK crate (few dependencies): it is about 150
  lines.
- **Settings by merge patch.** `update_settings` takes an RFC 7396 JSON merge
  patch, applied in core (`Settings::patched`), so an assistant changes one
  field without resending all of them.
- **Art as text.** `set_team_art` also takes LED art as rows of characters
  with a palette (`core::art::from_pixels`), so an assistant can draw
  without making an image file. Uploaded images go through the same decoder
  and limits as the admin page.
- Requests whose `Origin` names another site are refused, as for forms; the
  `Host` allowlist (DNS rebinding) covers these routes like every other.

## Consequences

- Claude Code (or any MCP client that runs on the owner's computer) can set
  up the whole sign; cloud-hosted assistants can't reach a home network,
  and that's fine.
- Keys are another secret in plain HTTP on the LAN, with the same accepted
  risk as the password and feed tokens (ADR-0015). They don't expire on
  their own; the page lists them with their dates.
- New admin features should add a tool in the same PR, or note why not.
