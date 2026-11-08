use crate::config::Value;

/// Values available for `{placeholder}` substitution in commands.
pub struct Ctx<'a> {
    pub rule: &'a str,
    pub source: &'a str,
    pub field: &'a str,
    pub value: &'a Value,
}

pub fn substitute(template: &str, ctx: &Ctx<'_>) -> String {
    template
        .replace("{rule}", ctx.rule)
        .replace("{source}", ctx.source)
        .replace("{field}", ctx.field)
        .replace("{value}", &ctx.value.as_str())
}

/// Run a command through `sh -c` (synchronous — fine for short commands).
pub fn run(spec: &str, ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let cmd = substitute(spec, ctx);
    tracing::info!("run: {cmd}");
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .status()?;
    if !status.success() {
        tracing::warn!("command exited with {status}");
    }
    Ok(())
}
