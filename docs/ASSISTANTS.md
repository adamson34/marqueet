# Let an assistant run your sign

Claude, or any assistant that speaks [MCP](https://modelcontextprotocol.io),
can set up a Marqueet for you: pick leagues and favorites, change the look,
give your team its own colors, words and a goal animation, try it on the
screen, follow a fantasy team, put a message on the sign. You ask in plain
words; it uses the same actions as the admin page.

## 1. Make a key

On the admin page (`http://<device>:7878/admin`), under **Assistants**, type
a name (`claude`, say) and press **Make key**. The page shows the key
(`mqk_…`) **once**: copy it then. Marqueet keeps only a hash of it.

A key can do anything the admin page does, except change the password or
make more keys. Revoke it under **Assistants** when you're done; it stops
working at once. Resetting the password from the SD card removes every key.

## 2. Connect

**Claude Code**, on a computer on the same network:

```sh
claude mcp add --transport http marqueet http://marqueet.local:7878/mcp \
  --header 'Authorization: Bearer mqk_…'
```

Then start `claude` and ask, for example:

- "What's on my Marqueet right now?"
- "Follow the NFL and the Premier League, and make LA baseball a favorite."
- "Draw a goal animation in my team's colors and test it on the screen."
- "Put HAPPY BIRTHDAY SAM on the ticker for today."

**Other assistants** that connect to MCP servers over HTTP use the same
address (`http://<device>:7878/mcp`) and header. An assistant that runs in
the cloud can't reach a device on your home network; use one that runs on
your computer.

## 3. Or call it from a script

Every tool is also plain HTTP, with the same key and JSON arguments:

```sh
curl http://marqueet.local:7878/api/tools \
  -H 'Authorization: Bearer mqk_…'                    # list the tools

curl -X POST http://marqueet.local:7878/api/tools/update_settings \
  -H 'Authorization: Bearer mqk_…' -H 'Content-Type: application/json' \
  -d '{"patch": {"takeovers": "favorites"}}'
```

Answers are JSON; a mistake is `400` with `{"error": "…"}`.

## The tools

| Tool | What it does |
| --- | --- |
| `status` | What's on screen: leagues, today's games and scores, recent alerts, feeds |
| `get_settings` / `update_settings` | Read settings; change some with a JSON merge patch (`null` resets a field) |
| `set_location` | Where the weather is for (a place name or `lat, lon`) |
| `list_leagues` / `list_teams` | What can be followed, and team ids for the tools below |
| `get_team_art` / `set_team_art` / `remove_team_art` | A team's colors, takeover words, LED art and logo |
| `test_takeover` | Play a sample takeover now, optionally as a team's |
| `list_feeds` / `create_feed` / `revoke_feed` | Feeds for your own scripts ([FEEDS.md](FEEDS.md)) |
| `post_to_feed` / `feed_alert` | Put content on the sign, flash it or take over |
| `find_fantasy` / `add_fantasy` / `remove_fantasy` | Follow a Sleeper fantasy team |
| `set_home_assistant` / `list_cameras` | Connect Home Assistant for home cameras ([ADR-0016](adr/0016-home-assistant-cameras.md)) |

`GET /api/tools` (or MCP's `tools/list`) gives each tool's arguments as a
JSON Schema.

### LED art without an image file

`set_team_art` takes art as an image (`image_base64`: a PNG, a PNG strip with
`frames_in_strip` frames side by side, or an animated GIF) or drawn as text,
one character per LED, which an assistant can write directly:

```json
{
  "team": "espn:mlb:1",
  "colors": { "primary": "#005a9c", "secondary": "#ffffff" },
  "words": { "home_run": { "headline": "IT'S GONE", "line": "SEE YOU LATER" } },
  "art": {
    "pixels": {
      "palette": { "B": "#005a9c", "W": "#ffffff" },
      "frames": [
        ["..BB..", ".BWWB.", "..BB.."],
        [".B..B.", "B.WW.B", ".B..B."]
      ]
    },
    "frame_ms": 120,
    "placement": "intro"
  }
}
```

`.` and space are off. Art is up to 96x48 LEDs and 48 frames;
`placement` is `above` (over the words), `intro` (on its own first, for one
loop of up to 4 seconds) or `behind` (dimmed behind the words). Only use art
you're allowed to use: Marqueet ships no team logos
([ADR-0013](adr/0013-bring-your-own-team-art.md)).

## Security

Keys travel in plain HTTP on your network, like the admin password and feed
tokens ([SECURITY.md](../SECURITY.md)): use them on a network you trust, and
revoke a key you no longer need. Requests from a web page on another site
are refused. The decisions are in
[ADR-0017](adr/0017-assistant-api-and-mcp.md).
