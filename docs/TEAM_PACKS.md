# Team colors, logos and team packs

Marqueet doesn't come with any team logos, and it never downloads them. If
you have images you're allowed to use, you can add them yourself: they show
next to the team on the LED ticker (as a small lit-up logo) and in the
widgets, and they stay on your device. You can also set a team's colors,
which then replace the colors from the scores everywhere (ticker, widgets,
score takeovers).

## One team at a time

On the admin page, under **Your team colors and logos**:

1. Pick the team.
2. Tick **Use my colors** and choose a main and second color, if you want.
3. Choose a logo: a PNG, ideally square with a see-through background. Big
   files are fine; Marqueet shrinks them to 128 pixels on the long side.
4. **Save team.**

To take a logo off again, tick **Remove this team's logo** and save, or use
**Remove** in the list to forget everything for that team.

## Team packs

A team pack is one JSON file with colors and logos for many teams. It's how
you move your setup to another Marqueet or share it with friends.

- **Download yours** from the admin page to back up what you've added.
- **Download a blank pack** to get every team you follow listed, with today's
  colors from the scores filled in, ready for you to edit.
- **Import** a pack to add everything in it. Teams already set up are
  replaced by what's in the pack; others are left alone.

### Format

```json
{
  "marqueet_team_pack": 1,
  "teams": [
    {
      "team": "espn:nfl:12",
      "name": "Kansas City Kingdom",
      "primary": "#d62a3c",
      "secondary": "#f5b32e",
      "logo_png": "iVBORw0KGgoAAAANSUhEUgAA..."
    }
  ]
}
```

| Field | Needed | What it is |
|---|---|---|
| `marqueet_team_pack` | yes | Format version; `1`. |
| `team` | yes | The team's id as Marqueet knows it (the blank pack lists them). |
| `name` | no | Shown on the admin page. |
| `primary`, `secondary` | no | `#rrggbb`. Colors apply only when `primary` is set. |
| `logo_png` | no | A PNG file, base64-encoded. |

Limits: 24 MB per pack, 600 teams, 2 MB and 4096 pixels per logo. A team
with neither colors nor a logo is skipped.

Only use logos you have the rights to use. Marqueet is not affiliated with
any league or team.
