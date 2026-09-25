# ESPN fixtures

Saved responses from ESPN's unofficial scoreboard endpoints
(`https://site.api.espn.com/apis/site/v2/sports/<sport>/<league>/scoreboard`),
used to test `normalize()` without the network.

| File | Source |
|---|---|
| `nfl.json`, `ncaaf.json`, `mlb.json`, `wnba.json`, `nhl.json`, `epl.json`, `mls.json` | **Real** captures from 2026-09-23 (about 4 PM ET), trimmed to at most 6 events each, preferring live, then final, then scheduled games. Otherwise unmodified. |
| `synthetic_live.json` | **Synthetic.** Real events from the captures above with their `status`, scores and `situation` edited to cover states that weren't live at capture time: NFL red zone, NFL final in OT, college halftime, NHL 2nd period and intermission, WNBA 4th quarter, EPL second half, MLB top of the 7th with runners on, MLB rain delay, MLB postponed, plus one malformed event (no competitors) that must be skipped. |

When ESPN changes shape, capture a fresh response for the affected league,
add it here, and add a test for the new case.

## Standings

| File | Source |
|---|---|
| `standings_nfl.json`, `standings_mlb.json`, `standings_nhl.json` | **Real** captures from 2026-09-23 of `https://site.api.espn.com/apis/v2/sports/<sport>/<league>/standings?level=3` (divisions). |
| `standings_epl.json`, `standings_wnba.json` | **Real** captures from 2026-09-23 of the same endpoint without `level` (one table / two conferences). |

All standings captures are trimmed to the group names, each entry's team id,
abbreviation and name, and the stats the normalizer reads. Entry order is
ESPN's (MLB's isn't sorted, which the tests rely on).

## Teams

| File | Source |
|---|---|
| `teams_nfl.json`, `teams_epl.json` | **Real** captures from 2026-09-24 of `https://site.api.espn.com/apis/site/v2/sports/<sport>/<league>/teams?limit=1000`, trimmed to each team's id, names, colors and `isActive`. |
