use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use serde::{de, Deserialize, Deserializer};

/// A single rule: a threshold condition plus commands to run on entering and,
/// optionally, leaving it.
#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub name: String,

    #[serde(flatten)]
    pub condition: Condition,

    /// Command run when the condition becomes true.
    pub run: String,

    /// Command run when the condition becomes false again (optional).
    #[serde(default)]
    pub on_exit: Option<String>,

    /// Margin applied on the leaving edge of a numeric threshold.
    #[serde(default)]
    pub hysteresis: f64,

    /// Minimum interval between two `run` executions of this rule.
    #[serde(default)]
    pub cooldown: Cooldown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Condition {
    /// Event source; for now only "battery" is supported.
    pub source: String,
    /// Event field, e.g. "percentage" or "state".
    pub field: String,
    #[serde(default = "default_op")]
    pub op: Comparator,
    pub value: Value,
}

fn default_op() -> Comparator {
    Comparator::Lt
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Comparator {
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "<=")]
    Le,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = ">=")]
    Ge,
    #[serde(rename = "==")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
}

/// A comparison value: numeric or textual (e.g. a battery state).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Number(f64),
    Text(String),
}

impl Value {
    pub fn as_str(&self) -> String {
        match self {
            Value::Number(n) => n.to_string(),
            Value::Text(s) => s.clone(),
        }
    }
}

/// A duration with human-friendly deserialization ("5s", "1m", or bare seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cooldown(pub Duration);

impl<'de> Deserialize<'de> for Cooldown {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(CooldownVisitor)
    }
}

struct CooldownVisitor;

impl<'de> de::Visitor<'de> for CooldownVisitor {
    type Value = Cooldown;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a duration like \"5s\" or a number of seconds")
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Cooldown, E> {
        if v < 0 {
            Err(E::custom("negative duration"))
        } else {
            Ok(Cooldown(Duration::from_secs(v as u64)))
        }
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Cooldown, E> {
        Ok(Cooldown(Duration::from_secs(v)))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Cooldown, E> {
        if v < 0.0 {
            Err(E::custom("negative duration"))
        } else {
            Ok(Cooldown(Duration::from_secs_f64(v)))
        }
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Cooldown, E> {
        parse_duration(v)
            .map(Cooldown)
            .ok_or_else(|| E::custom(format!("invalid duration: {v:?}")))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Cooldown, E> {
        parse_duration(&v)
            .map(Cooldown)
            .ok_or_else(|| E::custom(format!("invalid duration: {v:?}")))
    }
}

fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let idx = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(idx);
    let num: f64 = num.parse().ok()?;
    let secs = match unit.trim() {
        "" | "s" | "sec" | "secs" => num,
        "ms" | "msec" => num / 1000.0,
        "m" | "min" | "mins" => num * 60.0,
        "h" | "hr" | "hrs" => num * 3600.0,
        _ => return None,
    };
    if secs < 0.0 || !secs.is_finite() {
        return None;
    }
    Some(Duration::from_secs_f64(secs))
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

pub fn load(path: Option<&Path>) -> anyhow::Result<Config> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => default_path().context("no config path given")?,
    };
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config {}", path.display()))?;
    Ok(toml::from_str(&text)?)
}

fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("brelayd").join("config.toml"))
}
