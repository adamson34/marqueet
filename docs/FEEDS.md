# Feeds: put your own things on the sign

Any program that can send JSON over HTTP can add segments to the ticker, lines
to the crawl, and flashes or takeovers: stock prices, a build status, the
doorbell, the dishwasher. Scripts can be in any language; no Rust, no plugins
to compile.

## 1. Create a feed

On the admin page (`http://<device>:7878/admin`), under **Feeds**, enter a
name (1 to 32 of `a-z`, `0-9`, `-`, `_`) and press **Create**. The page shows
the feed's token **once**: copy it then. Marqueet keeps only a hash of it, so
it can't show it again; lost it? **New token** makes a new one (the old one
stops working). Each feed has its own token, and a token only works for its
own feed. **Revoke** deletes the feed, its token and whatever it's showing.
At most 20 feeds.

Send the token with every request:

```
Authorization: Bearer <token>
```

The feed API accepts requests from any address that can reach the server, but
only with a valid token. Treat tokens like passwords: they travel in plain
HTTP on your network (see [SECURITY.md](../SECURITY.md)).

`GET /api/feeds` lists the feeds and what each is showing (no tokens); it
needs the same access as the admin page.

## 2. Show something

`POST /api/feeds/<name>` replaces everything the feed shows:

```sh
curl -X POST http://marqueet.local:7878/api/feeds/stocks \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
        "segments": [
          {"id": "aapl", "text": "AAPL 189.20", "detail": "+1.2%", "color": "green"},
          {"id": "msft", "text": "MSFT 431.10", "detail": "-0.4%", "color": "red"}
        ],
        "crawl": [{"text": "Markets close at 4:00 PM"}],
        "ttl": 900
      }'
```

| Field | Meaning |
|---|---|
| `segments` | Up to 20 ticker segments (see below). |
| `crawl` | Up to 20 lines for the crawl under the ticker. |
| `ttl` | Seconds until the content disappears unless you post again (default 900; values outside 10 to 86400 are moved to the nearest end). Post on a schedule shorter than this. Content is kept in memory, so after a server restart it comes back with your script's next post (feeds and tokens are saved). |
| `position` | `end` (default): after the scores. `start`: right after the weather, at the front of the loop. |

A segment, simple form:

| Field | Meaning |
|---|---|
| `text` | The main text (required). |
| `detail` | A second, dimmer line, stacked under `text` on tall tickers. |
| `icon` | An LED icon: `sun`, `moon`, `cloud`, `partly_cloudy`, `partly_cloudy_night`, `fog`, `rain`, `snow`, `thunder`. |
| `color` | `#rrggbb`, `amber`, `red`, `green`, `blue`, `white`, `accent`, or `dim`. Default: the sign's LED color. |
| `id` | Stable id (`a-z0-9-_`), so alerts can flash this segment. Default: its position. |

Text is limited to 160 characters per segment, and a segment has at most 24 parts with gaps of at most 64 columns (so nothing can make the sign impossibly wide; the display also skips whatever doesn't fit its strip). Instead of `text`, a segment
can give `parts`, the display's own segment format (see
`crates/core/src/ticker.rs`), for full control.

A feed's content can be replaced at most once every 2 seconds (each change
makes the display draw the ticker again); sooner posts get 429.

Errors come back as `{"error": "..."}` with status 400 (bad content), 401
(unknown feed or wrong token) or 429 (too soon; see `Retry-After`).

## 3. Get attention

`POST /api/feeds/<name>/alert`:

```sh
curl -X POST http://marqueet.local:7878/api/feeds/home/alert \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"title": "Doorbell", "detail": "Front door", "level": "takeover", "color": "blue"}'
```

| Field | Meaning |
|---|---|
| `title` | Up to 40 characters. |
| `detail` | Optional, up to 80. |
| `level` | `flash` (default) blinks the feed's segment; `takeover` also fills the widget area for a few seconds. |
| `segment` | Which of the feed's segments to flash (its `id`); default the first. |
| `color` | Takeover color, as above. |

A feed can send one alert every 5 seconds and one takeover every 30. When
takeovers are turned off on the admin page, takeovers arrive as flashes.
"Only my favorite teams" applies to games; a feed's takeover (and a weather
warning's) still takes over, since you chose to send it.

## 4. Clear

`DELETE /api/feeds/<name>` removes the feed's content right away (the feed
and its token stay).

## A complete script (Python, no libraries)

```python
#!/usr/bin/env python3
"""Show a number on the Marqueet ticker every minute."""
import json, time, urllib.request

URL = "http://localhost:7878/api/feeds/example"
TOKEN = "paste the token from the admin page"

def post(body):
    req = urllib.request.Request(URL, data=json.dumps(body).encode(), method="POST",
                                 headers={"Authorization": f"Bearer {TOKEN}",
                                          "Content-Type": "application/json"})
    urllib.request.urlopen(req, timeout=5).read()

while True:
    visitors = 42  # get your number here
    post({"segments": [{"text": f"VISITORS {visitors}", "icon": "sun"}], "ttl": 180})
    time.sleep(60)
```
