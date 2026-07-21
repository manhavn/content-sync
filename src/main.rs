mod cli;
mod config;
mod models;
mod remote;
mod sync;
mod web;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = cli::Cli::parse();
    if let Err(e) = cli::run(cli).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
