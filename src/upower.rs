//! UPower-Anbindung über den System-D-Bus (via `zbus`).

use anyhow::Context;
use futures::StreamExt;
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;

use crate::config::Value;
use crate::trigger::Event;

const U_POWER: &str = "org.freedesktop.UPower";
const DEVICE_IFACE: &str = "org.freedesktop.UPower.Device";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

/// Map UPower `State` (uint32) to a human-readable name.
pub fn state_name(state: u32) -> &'static str {
    match state {
        1 => "Charging",
        2 => "Discharging",
        3 => "Empty",
        4 => "FullyCharged",
        5 => "PendingCharge",
        6 => "PendingDischarge",
        _ => "Unknown",
    }
}

#[derive(Clone)]
pub struct UpowerMonitor {
    proxy: PropertiesProxy<'static>,
}

impl UpowerMonitor {
    pub async fn connect() -> anyhow::Result<Self> {
        let conn = zbus::Connection::system()
            .await
            .context("connecting to system D-Bus")?;
        let proxy = PropertiesProxy::builder(&conn)
            .destination(U_POWER)?
            .path(DISPLAY_DEVICE)?
            .build()
            .await
            .context("building UPower PropertiesProxy")?;
        Ok(Self { proxy })
    }

    async fn percentage(&self) -> anyhow::Result<f64> {
        let value = self
            .proxy
            .get(InterfaceName::from_static_str(DEVICE_IFACE)?, "Percentage")
            .await?;
        Ok(value.downcast_ref::<f64>()?)
    }

    async fn state(&self) -> anyhow::Result<u32> {
        let value = self
            .proxy
            .get(InterfaceName::from_static_str(DEVICE_IFACE)?, "State")
            .await?;
        Ok(value.downcast_ref::<u32>()?)
    }

    /// Current percentage + state as events.
    pub async fn current_events(&self) -> anyhow::Result<Vec<Event>> {
        let pct = self.percentage().await?;
        let state = self.state().await?;
        Ok(vec![
            Event {
                source: "battery".into(),
                field: "percentage".into(),
                value: Value::Number(pct),
            },
            Event {
                source: "battery".into(),
                field: "state".into(),
                value: Value::Text(state_name(state).into()),
            },
        ])
    }
}

/// Emit current values once, then re-emit on every UPower change signal.
/// Calls `on_event` for each emitted event.
pub async fn run<F>(monitor: &UpowerMonitor, mut on_event: F) -> anyhow::Result<()>
where
    F: FnMut(Event),
{
    for event in monitor.current_events().await? {
        on_event(event);
    }

    let mut changes = monitor.proxy.receive_properties_changed().await?;
    while changes.next().await.is_some() {
        for event in monitor.current_events().await? {
            on_event(event);
        }
    }

    tracing::warn!("UPower change stream ended");
    Ok(())
}
