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
use marqueet_provider_nws::Nws;
use marqueet_provider_openmeteo::OpenMeteo;
use marqueet_provider_sleeper::Sleeper;
use marqueet_server::{AdminOptions, Policy, Providers, SettingsStore, device};

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

    /// Admin password, for headless installs. Without it, a password is
    /// created at first boot with the code shown on the device's screen.
    /// Prefer the environment variable: command lines are visible to other
    /// users.
    #[arg(long, env = "MARQUEET_ADMIN_PASSWORD", hide_env_values = true)]
    admin_password: Option<String>,

    /// Extra host names this server answers to, for example behind a reverse
    /// proxy. It always answers to its IP addresses, `localhost` and its own
    /// name (`marqueet.local`); other names are refused.
    #[arg(long = "allowed-host", env = "MARQUEET_ALLOWED_HOSTS", value_delimiter = ',', value_name = "NAME")]
    allowed_hosts: Vec<String>,

    /// If this file exists (e.g. on the boot partition), forget the admin
    /// password and go back to first-boot setup.
    #[arg(long, value_name = "PATH")]
    reset_file: Option<PathBuf>,

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
    let db = SettingsStore::open(&cli.db)?;
    if let Some(path) = &cli.reset_file {
        device::apply_reset_file(path, &db)?;
    }
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
    let setup_urls = device::setup_urls(cli.listen, device::hostname().as_deref(), device::lan_ip());
    log::info!("admin page at http://{}/admin", cli.listen);

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
    let providers = Providers {
        scores: Arc::new(provider),
        weather: Some(Arc::new(OpenMeteo::new()?)),
        weather_alerts: Some(Arc::new(Nws::new()?)),
        fantasy: Some(Arc::new(Sleeper::new()?.with_cache_file(cli.db.with_file_name("sleeper-players.json")))),
    };
    let admin = AdminOptions {
        password: admin_password,
        setup_urls,
        hostname: device::hostname(),
        extra_hosts: cli.allowed_hosts.clone(),
    };
    marqueet_server::run(listener, providers, settings, Policy::default(), Some(db), admin, shutdown).await?;
    Ok(())
}
