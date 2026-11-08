use std::collections::HashMap;
use std::time::Instant;

use crate::command;
use crate::config::{Config, Rule};
use crate::state::State;
use crate::trigger::{Action, Event, RuleRuntime};

/// Holds the active rule set and their runtime state. Wrapped in
/// `Arc<RwLock<..>>` so the event loop and the SIGHUP reload can share it.
pub struct Engine {
    rules: Vec<Rule>,
    runtimes: HashMap<String, RuleRuntime>,
}

impl Engine {
    pub fn new(cfg: &Config, persisted: &State) -> Self {
        let runtimes = cfg
            .rules
            .iter()
            .map(|r| {
                let inside = persisted.inside.get(&r.name).copied().unwrap_or(false);
                (r.name.clone(), RuleRuntime::new(inside))
            })
            .collect();
        Self {
            rules: cfg.rules.clone(),
            runtimes,
        }
    }

    /// Replace the rule set, keeping the `inside` flag for rules that still
    /// exist. New rules start outside; removed rules are dropped.
    pub fn reload(&mut self, cfg: &Config) {
        let runtimes = cfg
            .rules
            .iter()
            .map(|r| {
                let inside = self
                    .runtimes
                    .get(&r.name)
                    .map(RuleRuntime::inside)
                    .unwrap_or(false);
                (r.name.clone(), RuleRuntime::new(inside))
            })
            .collect();
        self.rules = cfg.rules.clone();
        self.runtimes = runtimes;
    }

    /// Feed an event to every matching rule. Returns true if any rule's
    /// `inside` flag changed (i.e. state should be persisted).
    pub fn handle_event(&mut self, event: &Event) -> bool {
        let now = Instant::now();
        let mut changed = false;

        for rule in &self.rules {
            if rule.condition.source != event.source || rule.condition.field != event.field {
                continue;
            }
            let Some(rt) = self.runtimes.get_mut(&rule.name) else {
                continue;
            };
            match rt.on_value(rule, &event.value, now) {
                Action::Enter => {
                    run_command(rule, event, &rule.run);
                    changed = true;
                }
                Action::Exit => {
                    if let Some(on_exit) = &rule.on_exit {
                        run_command(rule, event, on_exit);
                    }
                    changed = true;
                }
                Action::None => {}
            }
        }

        changed
    }

    /// Build a persistable `State` from the current runtime `inside` flags.
    pub fn snapshot_state(&self) -> State {
        let inside = self
            .runtimes
            .iter()
            .map(|(name, rt)| (name.clone(), rt.inside()))
            .collect();
        State { inside }
    }
}

fn run_command(rule: &Rule, event: &Event, spec: &str) {
    let ctx = command::Ctx {
        rule: rule.name.as_str(),
        source: event.source.as_str(),
        field: event.field.as_str(),
        value: &event.value,
    };
    if let Err(e) = command::run(spec, &ctx) {
        tracing::error!("rule '{}' command failed: {e:#}", rule.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Comparator, Condition, Config, Cooldown, Rule, Value};
    use std::time::Duration;

    fn rule(name: &str) -> Rule {
        Rule {
            name: name.into(),
            condition: Condition {
                source: "battery".into(),
                field: "percentage".into(),
                op: Comparator::Lt,
                value: Value::Number(10.0),
            },
            run: "true".into(),
            on_exit: None,
            hysteresis: 0.0,
            cooldown: Cooldown(Duration::ZERO),
        }
    }

    #[test]
    fn reload_preserves_inside_for_kept_rules() {
        let cfg = Config {
            rules: vec![rule("a"), rule("b")],
        };
        let mut persisted = State::default();
        persisted.inside.insert("a".into(), true);
        persisted.inside.insert("b".into(), false);

        let mut engine = Engine::new(&cfg, &persisted);
        assert!(engine.runtimes["a"].inside());
        assert!(!engine.runtimes["b"].inside());

        engine.reload(&cfg);
        assert!(engine.runtimes["a"].inside());
        assert!(!engine.runtimes["b"].inside());
    }

    #[test]
    fn reload_adds_new_rules_outside_and_drops_removed() {
        let cfg = Config {
            rules: vec![rule("a")],
        };
        let mut persisted = State::default();
        persisted.inside.insert("a".into(), true);

        let mut engine = Engine::new(&cfg, &persisted);
        assert!(engine.runtimes["a"].inside());

        let new_cfg = Config {
            rules: vec![rule("b"), rule("c")],
        };
        engine.reload(&new_cfg);
        assert_eq!(engine.runtimes.len(), 2);
        assert!(!engine.runtimes.contains_key("a"));
        assert!(!engine.runtimes["b"].inside());
        assert!(!engine.runtimes["c"].inside());
    }

    #[test]
    fn snapshot_reflects_runtimes() {
        let cfg = Config {
            rules: vec![rule("a")],
        };
        let engine = Engine::new(&cfg, &State::default());
        let snap = engine.snapshot_state();
        assert!(!snap.inside["a"]);
    }
}
