mod command;
mod config;
mod engine;
mod state;
mod trigger;
mod upower;

use std::sync::{Arc, RwLock};

use anyhow::Context;
use clap::Parser;
use tokio::signal::unix::{signal, SignalKind};

use crate::engine::Engine;

/// Run commands in response to battery events (UPower/D-Bus).
#[derive(Parser, Debug)]
#[command(name = "brelayd", version, about)]
struct Cli {
    /// Path to the TOML config file.
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    /// Validate the config and print the parsed rules, then exit.
    #[arg(long)]
    check: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let cfg = config::load(cli.config.as_deref()).context("loading config")?;

    if cli.check {
        for rule in &cfg.rules {
            println!("{rule:#?}");
        }
        return Ok(());
    }

    if cfg.rules.is_empty() {
        tracing::warn!("no rules defined");
    }

    let persisted = state::State::load();
    let engine = Arc::new(RwLock::new(Engine::new(&cfg, &persisted)));
    let monitor = Arc::new(upower::UpowerMonitor::connect().await?);

    // SIGHUP → reload config and immediately re-evaluate the new rules.
    {
        let engine = Arc::clone(&engine);
        let monitor = Arc::clone(&monitor);
        let config_path = cli.config.clone();
        tokio::spawn(async move {
            let mut hangup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("failed to install SIGHUP handler: {e:#}");
                    return;
                }
            };
            loop {
                hangup.recv().await;
                tracing::info!("SIGHUP received, reloading config");
                match config::load(config_path.as_deref()) {
                    Ok(new_cfg) => match reload(&engine, &monitor, &new_cfg).await {
                        Ok(()) => {
                            tracing::info!("config reloaded ({} rule(s))", new_cfg.rules.len())
                        }
                        Err(e) => tracing::error!("reload failed: {e:#}"),
                    },
                    Err(e) => tracing::error!("reload: failed to load config: {e:#}"),
                }
            }
        });
    }

    upower::run(&monitor, {
        let engine = Arc::clone(&engine);
        move |event| {
            let mut engine = engine.write().unwrap();
            if engine.handle_event(&event) {
                engine.snapshot_state().save();
            }
        }
    })
    .await
}

/// Reload the engine's rule set and immediately evaluate the current battery
/// values against the new rules (so a freshly added rule fires right away).
async fn reload(
    engine: &Arc<RwLock<Engine>>,
    monitor: &Arc<upower::UpowerMonitor>,
    cfg: &config::Config,
) -> anyhow::Result<()> {
    {
        let mut engine = engine.write().unwrap();
        engine.reload(cfg);
    }

    let events = monitor.current_events().await?;

    let mut engine = engine.write().unwrap();
    let mut changed = false;
    for event in &events {
        changed |= engine.handle_event(event);
    }
    if changed {
        engine.snapshot_state().save();
    }
    Ok(())
}
