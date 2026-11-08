mod command;
mod config;
mod state;
mod trigger;
mod upower;

use std::collections::HashMap;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;

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

    // Restore persisted rule state and build the per-rule runtime.
    let mut state = state::State::load();
    let mut runtimes: HashMap<String, trigger::RuleRuntime> = cfg
        .rules
        .iter()
        .map(|r| {
            let inside = state.inside.get(&r.name).copied().unwrap_or(false);
            (r.name.clone(), trigger::RuleRuntime::new(inside))
        })
        .collect();

    tracing::info!(
        "loaded {} rule(s), {} currently inside",
        cfg.rules.len(),
        runtimes.values().filter(|rt| rt.inside()).count()
    );

    let monitor = upower::UpowerMonitor::connect().await?;

    upower::run(monitor, |event| {
        if dispatch(&cfg, &mut runtimes, &event, &mut state) {
            state.save();
        }
    })
    .await
}

/// Feed an event to all matching rules, run their commands, and update state.
/// Returns true if any rule's `inside` flag changed (i.e. state should be saved).
fn dispatch(
    cfg: &config::Config,
    runtimes: &mut HashMap<String, trigger::RuleRuntime>,
    event: &trigger::Event,
    state: &mut state::State,
) -> bool {
    let now = Instant::now();
    let mut changed = false;

    for rule in &cfg.rules {
        if rule.condition.source != event.source || rule.condition.field != event.field {
            continue;
        }

        let rt = runtimes.get_mut(&rule.name).expect("runtime exists");
        match rt.on_value(rule, &event.value, now) {
            trigger::Action::Enter => {
                let ctx = command::Ctx {
                    rule: rule.name.as_str(),
                    source: event.source.as_str(),
                    field: event.field.as_str(),
                    value: &event.value,
                };
                if let Err(e) = command::run(&rule.run, &ctx) {
                    tracing::error!("rule '{}' run failed: {e:#}", rule.name);
                }
                state.inside.insert(rule.name.clone(), true);
                changed = true;
            }
            trigger::Action::Exit => {
                if let Some(on_exit) = &rule.on_exit {
                    let ctx = command::Ctx {
                        rule: rule.name.as_str(),
                        source: event.source.as_str(),
                        field: event.field.as_str(),
                        value: &event.value,
                    };
                    if let Err(e) = command::run(on_exit, &ctx) {
                        tracing::error!("rule '{}' on_exit failed: {e:#}", rule.name);
                    }
                }
                state.inside.insert(rule.name.clone(), false);
                changed = true;
            }
            trigger::Action::None => {}
        }
    }

    changed
}
