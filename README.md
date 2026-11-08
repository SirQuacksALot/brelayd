<img src="avatar.svg" width="150" alt="brelayd avatar" align="left" />

### brelayd

![License](https://img.shields.io/github/license/SirQuacksALot/brelayd?label=License&style=flat-square&color=d97757&labelColor=c15f3c)
![CI](https://img.shields.io/github/actions/workflow/status/SirQuacksALot/brelayd/ci.yml?label=CI&style=flat-square&color=d97757&labelColor=c15f3c)
![Version](https://img.shields.io/github/v/tag/SirQuacksALot/brelayd?label=Version&style=flat-square&color=d97757&labelColor=c15f3c)

A small, event-driven daemon that runs commands in response to battery events. It subscribes to UPower on the system D-Bus and fires a configurable command when a battery condition is crossed — dimming the screen below a charge threshold and restoring it once charging resumes, for example. Rules are defined in TOML, with edge semantics, hysteresis and cooldown, and state that persists across restarts. No polling, no timers.

<br clear="left" />

## A rule looks like this

```toml
[[rule]]
name = "low-battery-dim"
source = "battery"
field = "percentage"
op = "<"
value = 10

run = "brightnessctl set 15%"        # when the condition becomes true
on_exit = "brightnessctl set 100%"   # when it becomes false again (optional)

hysteresis = 2      # leave only above value + hysteresis
cooldown = "5s"     # minimum time between two "run" executions
```

## How it works

- **Event-driven** — listens to UPower on the system D-Bus and reacts only when the battery actually changes. No timers, no polling.
- **Edges, not levels** — `run` fires once on the *edge* into the condition, `on_exit` once on the edge out. No repeated firing while the condition stays true.
- **Hysteresis & cooldown** — prevent flapping around a threshold and command spam.
- **Persistent state** — remembers which rules are "inside", so a restart neither re-fires nor forgets to restore.
- **Placeholders** — commands may use `{value}`, `{field}`, `{source}` and `{rule}`.
- **Live reload** — `SIGHUP` re-reads the config and re-evaluates new rules immediately, without a restart.

## Configuration

Rules live in a TOML file, by default at `~/.config/brelayd/config.toml`. The
location follows XDG conventions: `$XDG_CONFIG_HOME/brelayd/config.toml` when
that variable is set, `~/.config/brelayd/config.toml` otherwise.

To use a different file, pass `--config` (or `-c`):

```sh
brelayd --config /path/to/config.toml
```

The systemd unit uses the default location. To point it elsewhere, add a
drop-in override via `systemctl --user edit brelayd`:

```ini
[Service]
ExecStart=/usr/bin/brelayd --config %h/.config/brelayd/config.toml
```

(`%h` expands to your home directory.)

Validate a config without running the daemon:

```sh
brelayd --check --config config.example.toml
```

Reload the config without restarting (`SIGHUP`):

```sh
systemctl --user reload brelayd
```

Newly added rules are evaluated immediately.

### Rule reference

| Field | Type | Description |
|---|---|---|
| `name` | string | unique rule id (used for persisted state) |
| `source` | string | event source — currently only `"battery"` |
| `field` | string | `"percentage"` (number) or `"state"` (text) |
| `op` | string | `<`, `<=`, `>`, `>=`, `==`, `!=` |
| `value` | number/string | threshold, or a state name like `"Discharging"` |
| `run` | string | shell command when the condition becomes true |
| `on_exit` | string (optional) | shell command when it becomes false again |
| `hysteresis` | number (optional) | margin on the leaving edge (default `0`) |
| `cooldown` | string (optional) | minimum interval between `run`s, e.g. `"5s"` (default `0`) |

`state` values: `Charging`, `Discharging`, `Empty`, `FullyCharged`, `PendingCharge`, `PendingDischarge`, `Unknown`.

## Install

| Method | Command |
|---|---|
| Cargo | `cargo install --path .` |
| Nix | `nix run github:SirQuacksALot/brelayd` |
| Arch | `makepkg -si` (see `PKGBUILD`) |
| Debian/RPM | download from the latest release |

A systemd user unit lives at [`contrib/brelayd.service`](contrib/brelayd.service).
The packages install it to `/usr/lib/systemd/user/brelayd.service` but do not
enable or start it — run this once after installing:

```sh
systemctl --user enable --now brelayd
```

## Development

```sh
cargo test                                     # unit tests
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo run -- --check --config config.example.toml   # validate config / dry-run
```

CI runs `fmt`, `clippy`, `test` and `nix flake check` on every push.

## License

[MIT](LICENSE)
