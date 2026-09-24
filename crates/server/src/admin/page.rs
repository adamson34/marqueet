//! The admin page as plain server-rendered HTML. Pure: everything it shows is
//! passed in, and every interpolated string goes through [`esc`].

use std::fmt::Write as _;

use chrono::{DateTime, FixedOffset, Utc};
use marqueet_core::alert::{Alert, AlertLevel};
use marqueet_core::config::ScrollMode;
use marqueet_core::provider::LeagueInfo;
use marqueet_core::settings::{Settings, TakeoverPolicy, WidgetKind};
use marqueet_core::sports::{LeagueId, TeamId};
use marqueet_core::weather::Units;

/// A team that can be picked as a favorite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamChoice {
    pub id: TeamId,
    pub league: LeagueId,
    pub name: String,
}

/// A feed on the admin page.
#[derive(Clone, Debug)]
pub struct FeedRow {
    pub name: String,
    pub token: String,
    pub segments: usize,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Fetch health for one followed league.
#[derive(Clone, Debug)]
pub struct LeagueHealth {
    pub id: LeagueId,
    pub games: usize,
    pub stale: bool,
    pub failures: u32,
    pub last_error: Option<String>,
    pub last_success: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    None,
    Saved,
    Error(String),
}

#[derive(Debug)]
pub struct View<'a> {
    pub settings: &'a Settings,
    pub leagues: &'a [LeagueInfo],
    pub teams: &'a [TeamChoice],
    pub health: &'a [LeagueHealth],
    pub alerts: &'a [Alert],
    /// Time zone names for the picker.
    pub zones: &'a [String],
    /// Feeds with their tokens.
    pub feeds: &'a [FeedRow],
    /// This server's address as the browser sees it, for the feed example.
    pub host: &'a str,
    pub notice: Notice,
    /// Logged in over the network (shows "Log out").
    pub remote: bool,
    pub tz: FixedOffset,
    pub now: DateTime<Utc>,
}

/// Escapes text for HTML element content and quoted attribute values.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn checked(on: bool) -> &'static str {
    if on { " checked" } else { "" }
}

fn selected(on: bool) -> &'static str {
    if on { " selected" } else { "" }
}

fn head(title: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{}</title><link rel=\"stylesheet\" href=\"/admin/admin.css\">\
         <script src=\"/admin/admin.js\" defer></script></head><body>",
        esc(title)
    )
}

const BRAND: &str = "<header class=\"top\"><span class=\"brand\">MARQUEET</span><span class=\"sub\">admin</span>";

fn ago(t: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let s = (now - t).num_seconds().max(0);
    match s {
        0..60 => format!("{s}s ago"),
        60..3600 => format!("{}m ago", s / 60),
        _ => format!("{}h ago", s / 3600),
    }
}

fn league_name<'a>(leagues: &'a [LeagueInfo], id: &'a LeagueId) -> &'a str {
    leagues.iter().find(|l| &l.id == id).map_or(id.as_str(), |l| l.name.as_str())
}

pub fn render(v: &View<'_>) -> String {
    let s = v.settings;
    let mut h = head("Marqueet admin");
    h.push_str(BRAND);
    if v.remote {
        h.push_str("<form method=\"post\" action=\"/logout\" class=\"logout\"><button>Log out</button></form>");
    }
    h.push_str("</header><main>");
    match &v.notice {
        Notice::None => {}
        Notice::Saved => {
            h.push_str("<p class=\"notice ok\" role=\"status\">Saved. The display updates in a moment.</p>")
        }
        Notice::Error(e) => {
            let _ = write!(h, "<p class=\"notice err\" role=\"alert\">Not saved: {}</p>", esc(e));
        }
    }
    h.push_str("<form method=\"post\" action=\"/admin\" id=\"settings\">");

    // Leagues: followed first, in ticker order, then the rest.
    h.push_str(
        "<section><h2>Leagues</h2><p class=\"hint\">Tick the leagues to follow. Drag (or use the numbers) \
         to set the ticker order.</p><ol class=\"leagues\" id=\"leagues\">",
    );
    let mut ordered: Vec<&LeagueInfo> =
        s.leagues.iter().filter_map(|id| v.leagues.iter().find(|l| &l.id == id)).collect();
    ordered.extend(v.leagues.iter().filter(|l| !s.leagues.contains(&l.id)));
    for (i, l) in ordered.iter().enumerate() {
        let id = esc(l.id.as_str());
        let _ = write!(
            h,
            "<li><span class=\"grip\" aria-hidden=\"true\">⋮⋮</span>\
             <label><input type=\"checkbox\" name=\"league\" value=\"{id}\"{}> {}</label>\
             <input class=\"order\" type=\"number\" name=\"order_{id}\" value=\"{}\" min=\"1\" aria-label=\"Order for {}\">\
             </li>",
            checked(s.leagues.contains(&l.id)),
            esc(&l.name),
            i + 1,
            esc(&l.name),
        );
    }
    h.push_str("</ol></section>");

    // Favorites: teams in today's games, plus saved favorites not playing.
    h.push_str(
        "<section><h2>Favorite teams</h2><p class=\"hint\">Favorites are listed first, and can be the only \
         teams whose big plays take over the screen. Teams appear here when they're on today's schedule.</p>",
    );
    let mut any = false;
    for league in &s.leagues {
        let teams: Vec<&TeamChoice> = v.teams.iter().filter(|t| &t.league == league).collect();
        if teams.is_empty() {
            continue;
        }
        any = true;
        let picked = teams.iter().filter(|t| s.is_favorite(&t.id)).count();
        let _ = write!(
            h,
            "<details class=\"teams\"{}><summary>{}<span class=\"count\">{}</span></summary><div>",
            if picked > 0 { " open" } else { "" },
            esc(league_name(v.leagues, league)),
            if picked > 0 { format!("{picked} picked") } else { format!("{} teams", teams.len()) },
        );
        for t in teams {
            let _ = write!(
                h,
                "<label><input type=\"checkbox\" name=\"favorite\" value=\"{}\"{}> {}</label>",
                esc(&t.id.0),
                checked(s.is_favorite(&t.id)),
                esc(&t.name),
            );
        }
        h.push_str("</div></details>");
    }
    let offstage: Vec<&TeamId> = s.favorites.iter().filter(|f| !v.teams.iter().any(|t| &t.id == *f)).collect();
    if !offstage.is_empty() {
        any = true;
        h.push_str("<details class=\"teams\" open><summary>Not playing today</summary><div>");
        for f in offstage {
            let _ = write!(
                h,
                "<label><input type=\"checkbox\" name=\"favorite\" value=\"{0}\" checked> {0}</label>",
                esc(&f.0)
            );
        }
        h.push_str("</div></details>");
    }
    if !any {
        h.push_str("<p class=\"empty\">No games loaded yet. Check back once the scoreboard has data.</p>");
    }
    h.push_str("</section>");

    // Takeovers.
    h.push_str("<section><h2>Big-play takeovers</h2><div class=\"choices\">");
    for (value, policy, label) in [
        ("all", TakeoverPolicy::All, "Every touchdown, home run and goal"),
        ("favorites", TakeoverPolicy::Favorites, "Favorites only (others flash the ticker)"),
        ("off", TakeoverPolicy::Off, "Off (just flash the ticker)"),
    ] {
        let _ = write!(
            h,
            "<label><input type=\"radio\" name=\"takeovers\" value=\"{value}\"{}> {label}</label>",
            checked(s.takeovers == policy)
        );
    }
    h.push_str("</div></section>");

    // Widgets.
    h.push_str("<section><h2>Widgets</h2><div class=\"row\">");
    for (slot, name) in [(0, "Left"), (1, "Right")] {
        let current = s.widgets.get(slot).copied();
        let _ = write!(h, "<label>{name} <select name=\"widget_{slot}\">");
        for (value, kind, label) in [
            ("game_of_the_day", WidgetKind::GameOfTheDay, "Game of the day"),
            ("scores", WidgetKind::Scores, "Scores"),
            ("standings", WidgetKind::Standings, "Standings"),
            ("weather", WidgetKind::Weather, "Weather"),
        ] {
            let _ = write!(h, "<option value=\"{value}\"{}>{label}</option>", selected(current == Some(kind)));
        }
        h.push_str("</select></label>");
    }
    h.push_str("</div></section>");

    // Display look.
    let d = &s.display;
    let _ = write!(
        h,
        "<section><h2>Display</h2><div class=\"grid\">\
         <label>LED color <input type=\"color\" name=\"led_color\" value=\"{}\"></label>\
         <label>Ticker rows <input type=\"number\" name=\"ticker_rows\" value=\"{}\" min=\"9\" max=\"48\"></label>\
         <label>Ticker speed <input type=\"number\" name=\"ticker_speed\" value=\"{}\" min=\"1\" max=\"200\" step=\"any\"></label>\
         <label>Crawl speed <input type=\"number\" name=\"crawl_speed\" value=\"{}\" min=\"1\" max=\"200\" step=\"any\"></label>\
         <label>Glow <input type=\"range\" name=\"glow\" value=\"{}\" min=\"0\" max=\"2\" step=\"0.05\"></label>\
         <label>Flicker <input type=\"range\" name=\"flicker\" value=\"{}\" min=\"0\" max=\"1\" step=\"0.05\"></label>\
         </div><div class=\"choices inline\">\
         <label><input type=\"radio\" name=\"scroll_mode\" value=\"stepped\"{}> Stepped (like a real sign)</label>\
         <label><input type=\"radio\" name=\"scroll_mode\" value=\"smooth\"{}> Smooth</label></div></section>",
        d.led_color,
        d.ticker_rows,
        d.ticker_speed,
        d.crawl_speed,
        d.glow,
        d.flicker,
        checked(d.scroll_mode == ScrollMode::Stepped),
        checked(d.scroll_mode == ScrollMode::Smooth),
    );

    // Weather.
    let w = &s.weather;
    let _ = write!(
        h,
        "<section><h2>Weather</h2><p class=\"hint\">For the weather widget. Forecasts come from \
         <a href=\"https://open-meteo.com\">Open-Meteo</a> (CC BY 4.0); the location is sent to them only \
         while the weather is on screen (ticker or widget).</p>\
         <label class=\"wide\">Location <input name=\"location\" value=\"{}\" \
         placeholder=\"City, or latitude, longitude\" autocomplete=\"off\"></label>\
         <div class=\"choices inline\">\
         <label><input type=\"radio\" name=\"units\" value=\"fahrenheit\"{}> °F, mph</label>\
         <label><input type=\"radio\" name=\"units\" value=\"celsius\"{}> °C, km/h</label></div>\
         <div class=\"choices\"><label><input type=\"checkbox\" name=\"weather_ticker\"{}> \
         Show the weather in the ticker (with a heads-up when rain or snow is coming)</label>\
         <label><input type=\"checkbox\" name=\"weather_alerts\"{}> Severe weather alerts (US only, from the \
         National Weather Service): on the ticker while in effect, and warnings take over the screen</label></div></section>",
        esc(w.place.as_ref().map_or("", |p| p.name.as_str())),
        checked(w.units == Units::Fahrenheit),
        checked(w.units == Units::Celsius),
        checked(w.ticker),
        checked(w.alerts),
    );

    // Time zone and quiet hours.
    let _ = write!(
        h,
        "<section><h2>Time</h2><label class=\"wide\">Time zone \
         <input name=\"time_zone\" list=\"zones\" value=\"{}\" placeholder=\"Same as the device\" \
         autocomplete=\"off\" spellcheck=\"false\"></label><datalist id=\"zones\">",
        esc(s.time_zone.as_deref().unwrap_or("")),
    );
    for z in v.zones {
        let _ = write!(h, "<option value=\"{}\">", esc(z));
    }
    h.push_str("</datalist>");
    let q = s.quiet_hours;
    let _ = write!(
        h,
        "<h3>Quiet hours</h3><div class=\"row\">\
         <label><input type=\"checkbox\" name=\"quiet_enabled\"{}> Blank the screen</label>\
         <label>from <input type=\"time\" name=\"quiet_from\" value=\"{}\"></label>\
         <label>to <input type=\"time\" name=\"quiet_to\" value=\"{}\"></label></div></section>",
        checked(q.is_some()),
        q.map_or("23:00".into(), |q| q.from.format("%H:%M").to_string()),
        q.map_or("07:00".into(), |q| q.to.format("%H:%M").to_string()),
    );

    h.push_str("<div class=\"actions\"><button type=\"submit\" class=\"primary\">Save</button></div></form>");

    // Feeds (their own forms: they act at once, not on Save).
    h.push_str(
        "<section id=\"feeds\"><h2>Feeds</h2><p class=\"hint\">Let your own scripts put things on the \
         sign: stock prices, server status, the doorbell. Each feed gets a token; send it as \
         <code>Authorization: Bearer …</code>. See <code>docs/FEEDS.md</code> for the format.</p>",
    );
    if !v.feeds.is_empty() {
        h.push_str("<table class=\"feeds\"><thead><tr><th>Feed</th><th>On screen</th><th>Token</th><th></th></tr></thead><tbody>");
        for f in v.feeds {
            let state = match f.expires_at {
                Some(t) if t > v.now => {
                    let mins = (t - v.now).num_minutes().max(1);
                    format!(
                        "<span class=\"good\">{} segment{}</span>, {mins} min left",
                        f.segments,
                        if f.segments == 1 { "" } else { "s" }
                    )
                }
                _ => "idle".into(),
            };
            let _ = write!(
                h,
                "<tr><td>{0}</td><td>{state}</td><td><code class=\"token\">{1}</code></td><td>\
                 <form method=\"post\" action=\"/admin/feeds/revoke\"><input type=\"hidden\" name=\"name\" value=\"{0}\">\
                 <button>Revoke</button></form></td></tr>",
                esc(&f.name),
                esc(&f.token),
            );
        }
        h.push_str("</tbody></table>");
    }
    h.push_str(
        "<form method=\"post\" action=\"/admin/feeds\" class=\"row new-feed\"><label>New feed \
         <input name=\"name\" required pattern=\"[a-z0-9_\\-]{1,32}\" placeholder=\"stocks\" \
         title=\"1-32 of a-z, 0-9, - and _\"></label><button>Create</button></form>",
    );
    let (name, token) = v.feeds.first().map_or(("stocks", "TOKEN"), |f| (f.name.as_str(), f.token.as_str()));
    let _ = write!(
        h,
        "<details><summary>Example</summary><pre>curl -X POST http://{host}/api/feeds/{name} \\\n  \
         -H 'Authorization: Bearer {token}' \\\n  -H 'Content-Type: application/json' \\\n  \
         -d '{{\"segments\": [{{\"text\": \"AAPL 189.20\", \"detail\": \"+1.2%\", \"color\": \"green\"}}]}}'</pre></details></section>",
        host = esc(v.host),
        name = esc(name),
        token = esc(token),
    );

    // Status (read-only).
    h.push_str("<section class=\"status\"><h2>Status</h2><table><thead><tr><th>League</th><th>Games</th><th>Last update</th><th>State</th></tr></thead><tbody>");
    for l in v.health {
        let state = if l.stale {
            format!("<span class=\"bad\">stale</span> {}", esc(l.last_error.as_deref().unwrap_or("")))
        } else if l.failures > 0 {
            format!("<span class=\"warn\">retrying ({})</span>", l.failures)
        } else if l.last_success.is_some() {
            "<span class=\"good\">ok</span>".into()
        } else {
            "waiting".into()
        };
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{state}</td></tr>",
            esc(league_name(v.leagues, &l.id)),
            l.games,
            l.last_success.map_or("never".into(), |t| ago(t, v.now)),
        );
    }
    h.push_str("</tbody></table><h3>Recent plays</h3>");
    if v.alerts.is_empty() {
        h.push_str("<p class=\"empty\">Nothing yet.</p>");
    } else {
        h.push_str("<ul class=\"alerts\">");
        for a in v.alerts.iter().take(12) {
            let kind = if a.level == AlertLevel::Takeover { "takeover" } else { "flash" };
            let _ = write!(
                h,
                "<li><time>{}</time> <strong>{}</strong> {} <span class=\"tag\">{kind}</span></li>",
                a.created_at.with_timezone(&v.tz).format("%-I:%M %p"),
                esc(&a.title),
                esc(a.detail.as_deref().unwrap_or("")),
            );
        }
        h.push_str("</ul>");
    }
    h.push_str("</section></main></body></html>");
    h
}

pub fn login(error: bool) -> String {
    let mut h = head("Marqueet login");
    h.push_str(BRAND);
    h.push_str("</header><main class=\"narrow\"><form method=\"post\" action=\"/login\"><h2>Log in</h2>");
    if error {
        h.push_str("<p class=\"notice err\" role=\"alert\">Wrong password.</p>");
    }
    h.push_str(
        "<label>Admin password <input type=\"password\" name=\"password\" autocomplete=\"current-password\" \
         autofocus required></label><div class=\"actions\"><button class=\"primary\">Log in</button></div>\
         </form></main></body></html>",
    );
    h
}

pub fn forbidden() -> String {
    let mut h = head("Marqueet admin");
    h.push_str(BRAND);
    h.push_str(
        "</header><main class=\"narrow\"><h2>Admin is only open on the device</h2>\
         <p>To change settings from another computer, start the server with an admin password:</p>\
         <pre>MARQUEET_ADMIN_PASSWORD=… marqueet-server --listen 0.0.0.0:7878</pre></main></body></html>",
    );
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::sports::Sport;

    fn info(id: &str, name: &str) -> LeagueInfo {
        LeagueInfo { id: LeagueId::new(id), sport: Sport::Football, name: name.into() }
    }

    fn view<'a>(settings: &'a Settings, leagues: &'a [LeagueInfo], teams: &'a [TeamChoice]) -> View<'a> {
        View {
            settings,
            leagues,
            teams,
            health: &[],
            alerts: &[],
            zones: &[],
            feeds: &[],
            host: "marqueet.local:7878",
            notice: Notice::None,
            remote: false,
            tz: FixedOffset::east_opt(0).unwrap(),
            now: Utc::now(),
        }
    }

    #[test]
    fn escapes_everything_interpolated() {
        assert_eq!(esc(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
        let settings = Settings {
            leagues: vec![LeagueId::new("nfl")],
            favorites: vec![TeamId("\"><script>x</script>".into())],
            ..Settings::default()
        };
        let leagues = [info("nfl", "N<F>L")];
        let teams = [TeamChoice { id: TeamId("espn:nfl:1".into()), league: LeagueId::new("nfl"), name: "<b>".into() }];
        let mut v = view(&settings, &leagues, &teams);
        v.notice = Notice::Error("<img src=x>".into());
        let html = render(&v);
        assert!(!html.contains("<script>x") && !html.contains("<b>") && !html.contains("<img"));
        assert!(html.contains("N&lt;F&gt;L") && html.contains("&lt;img src=x&gt;"));
    }

    #[test]
    fn followed_leagues_come_first_in_order() {
        let settings = Settings { leagues: vec![LeagueId::new("mlb"), LeagueId::new("nfl")], ..Settings::default() };
        let leagues = [info("nfl", "NFL"), info("nhl", "NHL"), info("mlb", "MLB")];
        let html = render(&view(&settings, &leagues, &[]));
        let at = |needle: &str| html.find(needle).unwrap();
        assert!(at("value=\"mlb\" checked") < at("value=\"nfl\" checked"));
        assert!(at("value=\"nfl\" checked") < at("value=\"nhl\">"), "unfollowed leagues are unchecked, last");
        assert!(html.contains("name=\"order_mlb\" value=\"1\"") && html.contains("name=\"order_nhl\" value=\"3\""));
    }

    #[test]
    fn favorites_not_playing_stay_ticked() {
        let settings = Settings {
            leagues: vec![LeagueId::new("nfl")],
            favorites: vec![TeamId("espn:nfl:2".into()), TeamId("espn:nfl:9".into())],
            ..Settings::default()
        };
        let leagues = [info("nfl", "NFL")];
        let teams =
            [TeamChoice { id: TeamId("espn:nfl:2".into()), league: LeagueId::new("nfl"), name: "Bills".into() }];
        let html = render(&view(&settings, &leagues, &teams));
        assert!(html.contains("value=\"espn:nfl:2\" checked> Bills"));
        assert!(html.contains("Not playing today") && html.contains("value=\"espn:nfl:9\" checked"));
    }

    #[test]
    fn form_round_trips_through_the_parser() {
        // Every value the page renders must parse back to the same settings.
        let settings = Settings {
            leagues: vec![LeagueId::new("nfl"), LeagueId::new("mlb")],
            quiet_hours: Some(marqueet_core::settings::QuietHours {
                from: chrono::NaiveTime::from_hms_opt(23, 30, 0).unwrap(),
                to: chrono::NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
            }),
            time_zone: Some("America/Chicago".into()),
            ..Settings::default()
        }
        .sanitized();
        let leagues = [info("nfl", "NFL"), info("mlb", "MLB"), info("nhl", "NHL")];
        let html = render(&view(&settings, &leagues, &[]));
        let pairs = submitted(&html);
        let ids: Vec<LeagueId> = leagues.iter().map(|l| l.id.clone()).collect();
        let parsed = super::super::form::apply(&Settings::default(), &ids, &pairs).unwrap();
        assert_eq!(parsed, settings);
    }

    /// What a browser would submit for the rendered form (enough of HTML for
    /// our own markup: inputs and selects in the settings form).
    fn submitted(html: &str) -> Vec<(String, String)> {
        let form = &html[html.find("id=\"settings\"").unwrap()..html.find("</form>").unwrap()];
        let attr = |tag: &str, name: &str| {
            let key = format!("{name}=\"");
            tag.find(&key).map(|i| {
                let rest = &tag[i + key.len()..];
                rest[..rest.find('"').unwrap()].to_owned()
            })
        };
        let mut out = Vec::new();
        for tag in form.split('<').skip(1) {
            let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
            if tag.starts_with("input") {
                let (Some(name), value) = (attr(tag, "name"), attr(tag, "value")) else { continue };
                let kind = attr(tag, "type").unwrap_or_default();
                if (kind == "checkbox" || kind == "radio") && !tag.contains(" checked") {
                    continue;
                }
                out.push((name, value.unwrap_or_else(|| "on".into())));
            } else if tag.starts_with("select") {
                let name = attr(tag, "name").unwrap();
                let rest = &form[form.find(tag).unwrap()..];
                let opt = rest.split("<option").find(|o| o.contains(" selected")).unwrap();
                out.push((name, attr(opt, "value").unwrap()));
            }
        }
        out
    }
}
