use std::time::Instant;

use crate::config::{Comparator, Rule, Value};

/// A single value read from an event source.
#[derive(Debug, Clone)]
pub struct Event {
    pub source: String,
    pub field: String,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The condition just became true: run the rule's `run` command.
    Enter,
    /// The condition just became false: run the rule's `on_exit` command.
    Exit,
    None,
}

/// Per-rule runtime state deciding the edges (enter/exit) with hysteresis and
/// cooldown. `inside` is persisted across restarts; the rest is runtime-only.
#[derive(Debug, Clone)]
pub struct RuleRuntime {
    inside: bool,
    last_enter: Option<Instant>,
}

impl RuleRuntime {
    pub fn new(inside: bool) -> Self {
        Self {
            inside,
            last_enter: None,
        }
    }

    pub fn inside(&self) -> bool {
        self.inside
    }

    /// Feed a value and decide what to do.
    pub fn on_value(&mut self, rule: &Rule, value: &Value, now: Instant) -> Action {
        if !self.inside {
            if entered(rule.condition.op, value, &rule.condition.value) {
                if let Some(t) = self.last_enter {
                    if now.duration_since(t) < rule.cooldown.0 {
                        return Action::None;
                    }
                }
                self.inside = true;
                self.last_enter = Some(now);
                Action::Enter
            } else {
                Action::None
            }
        } else if exited(
            rule.condition.op,
            value,
            &rule.condition.value,
            rule.hysteresis,
        ) {
            self.inside = false;
            Action::Exit
        } else {
            Action::None
        }
    }
}

/// True when `value op threshold` holds (the "enter" condition).
pub fn entered(op: Comparator, value: &Value, threshold: &Value) -> bool {
    compare(op, value, threshold)
}

/// True when the rule should leave the condition, honoring hysteresis for
/// numeric thresholds.
pub fn exited(op: Comparator, value: &Value, threshold: &Value, hysteresis: f64) -> bool {
    match (value, threshold) {
        (Value::Number(a), Value::Number(b)) => match op {
            Comparator::Lt | Comparator::Le => *a >= b + hysteresis,
            Comparator::Gt | Comparator::Ge => *a <= b - hysteresis,
            Comparator::Eq => (*a - *b).abs() >= f64::EPSILON,
            Comparator::Ne => (*a - *b).abs() < f64::EPSILON,
        },
        (Value::Text(a), Value::Text(b)) => match op {
            Comparator::Eq => a != b,
            Comparator::Ne => a == b,
            _ => true,
        },
        _ => false,
    }
}

fn compare(op: Comparator, value: &Value, threshold: &Value) -> bool {
    match (value, threshold) {
        (Value::Number(a), Value::Number(b)) => match op {
            Comparator::Lt => a < b,
            Comparator::Le => a <= b,
            Comparator::Gt => a > b,
            Comparator::Ge => a >= b,
            Comparator::Eq => (*a - *b).abs() < f64::EPSILON,
            Comparator::Ne => (*a - *b).abs() >= f64::EPSILON,
        },
        (Value::Text(a), Value::Text(b)) => match op {
            Comparator::Eq => a == b,
            Comparator::Ne => a != b,
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::config::{Condition, Cooldown};

    fn rule(op: Comparator, value: Value, hysteresis: f64, cooldown: Duration) -> Rule {
        Rule {
            name: "t".into(),
            condition: Condition {
                source: "battery".into(),
                field: "percentage".into(),
                op,
                value,
            },
            run: "true".into(),
            on_exit: None,
            hysteresis,
            cooldown: Cooldown(cooldown),
        }
    }

    fn t(secs: u64) -> Instant {
        Instant::now() + Duration::from_secs(secs)
    }

    #[test]
    fn enter_on_crossing_below_threshold() {
        let r = rule(Comparator::Lt, Value::Number(10.0), 2.0, Duration::ZERO);
        let mut rt = RuleRuntime::new(false);
        assert_eq!(rt.on_value(&r, &Value::Number(11.0), t(0)), Action::None);
        assert_eq!(rt.on_value(&r, &Value::Number(9.0), t(1)), Action::Enter);
        assert!(rt.inside());
    }

    #[test]
    fn no_double_enter_while_inside() {
        let r = rule(Comparator::Lt, Value::Number(10.0), 2.0, Duration::ZERO);
        let mut rt = RuleRuntime::new(false);
        rt.on_value(&r, &Value::Number(9.0), t(0));
        assert_eq!(rt.on_value(&r, &Value::Number(5.0), t(1)), Action::None);
    }

    #[test]
    fn exit_respects_hysteresis() {
        let r = rule(Comparator::Lt, Value::Number(10.0), 2.0, Duration::ZERO);
        let mut rt = RuleRuntime::new(false);
        rt.on_value(&r, &Value::Number(9.0), t(0)); // enter
        assert_eq!(rt.on_value(&r, &Value::Number(11.0), t(1)), Action::None);
        assert_eq!(rt.on_value(&r, &Value::Number(12.5), t(2)), Action::Exit);
        assert!(!rt.inside());
    }

    #[test]
    fn cooldown_suppresses_rapid_reenter() {
        let r = rule(
            Comparator::Lt,
            Value::Number(10.0),
            0.0,
            Duration::from_secs(5),
        );
        let mut rt = RuleRuntime::new(false);
        assert_eq!(rt.on_value(&r, &Value::Number(9.0), t(0)), Action::Enter);
        assert_eq!(rt.on_value(&r, &Value::Number(11.0), t(1)), Action::Exit);
        assert_eq!(rt.on_value(&r, &Value::Number(9.0), t(2)), Action::None);
        assert!(!rt.inside());
        assert_eq!(rt.on_value(&r, &Value::Number(9.0), t(7)), Action::Enter);
    }

    #[test]
    fn text_equality_condition() {
        let r = rule(
            Comparator::Eq,
            Value::Text("Discharging".into()),
            0.0,
            Duration::ZERO,
        );
        let mut rt = RuleRuntime::new(false);
        assert_eq!(
            rt.on_value(&r, &Value::Text("Charging".into()), t(0)),
            Action::None
        );
        assert_eq!(
            rt.on_value(&r, &Value::Text("Discharging".into()), t(1)),
            Action::Enter
        );
        assert_eq!(
            rt.on_value(&r, &Value::Text("Charging".into()), t(2)),
            Action::Exit
        );
    }
}
