//! `marqueet-server`: fetches live scores and feeds the display.
//!
//! ```text
//! marqueet-server                              # saved (or default) settings on 127.0.0.1:7878
//! marqueet-server --leagues nfl,mlb,nhl,epl    # override and save the league list
//! marqueet-server --db /var/lib/marqueet/marqueet.db
//! MARQUEET_ADMIN_PASSWORD=… marqueet-server --listen 0.0.0.0:7878   # admin page on the LAN
//! curl localhost:7878/api/games | jq .status
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use marqueet_core::protocol::DEFAULT_PORT;
use marqueet_core::provider::DataProvider;
use marqueet_core::sports::LeagueId;
use marqueet_provider_espn::EspnProvider;
use marqueet_server::{Policy, SettingsStore};

#[derive(Debug, Parser)]
#[command(name = "marqueet-server", version, about = "Polls live scores and feeds the Marqueet display")]
struct Cli {
    /// Address to listen on. The display runs on the same device, so the
    /// default is loopback only.
    #[arg(long, default_value_t = SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)))]
    listen: SocketAddr,

    /// Comma-separated leagues, in ticker order. Overrides (and saves) the
    /// stored league list; without it the saved settings are used.
    #[arg(long, value_delimiter = ',')]
    leagues: Option<Vec<String>>,

    /// SQLite database for settings.
    #[arg(long, default_value = "marqueet.db")]
    db: PathBuf,

    /// Password for the admin page from other computers. Without it the
    /// admin page only works on the device itself. Prefer the environment
    /// variable: command lines are visible to other users.
    #[arg(long, env = "MARQUEET_ADMIN_PASSWORD", hide_env_values = true)]
    admin_password: Option<String>,

    /// Print the supported leagues and exit.
    #[arg(long)]
    list_leagues: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    let provider = EspnProvider::new()?;
    let supported = provider.leagues();

    if cli.list_leagues {
        for l in &supported {
            println!("{:<6} {}", l.id, l.name);
        }
        return Ok(());
    }

    let admin_password = cli.admin_password.filter(|p| !p.is_empty());
    if let Some(p) = &admin_password
        && p.chars().count() < 8
    {
        return Err("the admin password must be at least 8 characters".into());
    }
    if !cli.listen.ip().is_loopback() && admin_password.is_none() {
        return Err(format!(
            "listening on {} would expose the admin page to the network; set MARQUEET_ADMIN_PASSWORD \
             (or --admin-password), or listen on 127.0.0.1",
            cli.listen
        )
        .into());
    }

    let db = SettingsStore::open(&cli.db)?;
    let mut settings = db.load()?.unwrap_or_default();
    if let Some(leagues) = &cli.leagues {
        settings.leagues = leagues.iter().map(|l| LeagueId::new(l.trim().to_lowercase())).collect();
        settings = settings.sanitized();
        db.save(&settings)?;
    }
    let leagues = settings.leagues.clone();
    if let Some(bad) = leagues.iter().find(|l| !supported.iter().any(|s| &s.id == *l)) {
        return Err(format!("unknown league {bad:?}; see --list-leagues").into());
    }
    log::info!("settings from {}", cli.db.display());
    log::info!(
        "admin page at http://{}/admin{}",
        cli.listen,
        if admin_password.is_some() { " (password required from other computers)" } else { " (this device only)" }
    );

    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    log::info!(
        "listening on {} (display feed at ws://{}/ws); leagues: {}",
        cli.listen,
        cli.listen,
        leagues.iter().map(LeagueId::as_str).collect::<Vec<_>>().join(",")
    );
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        log::info!("shutting down");
    };
    marqueet_server::run(listener, Arc::new(provider), settings, Policy::default(), Some(db), admin_password, shutdown)
        .await?;
    Ok(())
}
