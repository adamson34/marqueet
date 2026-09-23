//! `marqueet-server`: fetches live scores and feeds the display.
//!
//! ```text
//! marqueet-server                              # default leagues on 127.0.0.1:7878
//! marqueet-server --leagues nfl,mlb,nhl,epl
//! curl localhost:7878/api/games | jq .status
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use marqueet_core::protocol::DEFAULT_PORT;
use marqueet_core::provider::DataProvider;
use marqueet_core::sports::LeagueId;
use marqueet_provider_espn::EspnProvider;
use marqueet_server::Policy;

const DEFAULT_LEAGUES: &str = "nfl,ncaaf,mlb,nba,wnba,nhl,mls,epl";

#[derive(Debug, Parser)]
#[command(name = "marqueet-server", version, about = "Polls live scores and feeds the Marqueet display")]
struct Cli {
    /// Address to listen on. The display runs on the same device, so the
    /// default is loopback only.
    #[arg(long, default_value_t = SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)))]
    listen: SocketAddr,

    /// Comma-separated leagues, in ticker order.
    #[arg(long, default_value = DEFAULT_LEAGUES, value_delimiter = ',')]
    leagues: Vec<String>,

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

    let leagues: Vec<LeagueId> = cli.leagues.iter().map(|l| LeagueId::new(l.trim().to_lowercase())).collect();
    if let Some(bad) = leagues.iter().find(|l| !supported.iter().any(|s| &s.id == *l)) {
        return Err(format!("unknown league {bad:?}; see --list-leagues").into());
    }

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
    marqueet_server::run(listener, Arc::new(provider), leagues, Policy::default(), shutdown).await?;
    Ok(())
}
