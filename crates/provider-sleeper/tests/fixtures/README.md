# Sleeper fixtures

Responses from Sleeper's public read-only API (`https://api.sleeper.app/v1`),
captured on 2026-09-23.

| File | Source |
|---|---|
| `state.json` | **Real**, unmodified: `/state/nfl`. |
| `league.json`, `user_leagues.json`, `users.json`, `rosters.json` | **Real league, anonymized**: a completed 2025 12-team league. League, user and owner ids are replaced with fake ones; the league is "Example League", managers are `manager1`..`manager12`, team names are `Team N` (every third manager has none, as happens in real leagues). Only fields we read are kept. |
| `matchups_week3.json` | **Real**, unmodified: that league's week 3 (roster ids, NFL player ids, points). |
| `players_subset.json` | **Real**, trimmed: the `/players/nfl` entries for the week 3 starters, only the fields we read. The full file is about 15 MB. |
| `user.json` | **Synthetic**: a `/user/<name>` response for `example`. |

No real person's name or Sleeper account appears in these files.
