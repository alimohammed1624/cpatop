# cpatop

A btop-style terminal dashboard for [CPA Usage Keeper](https://github.com/Willxup/cpa-usage-keeper)
(the usage/cost tracker for CLIProxyAPI), written in Rust with [ratatui](https://ratatui.rs).

It mirrors the Keeper web dashboard in your terminal: totals and trends, realtime
throughput, per-model / per-key / per-credential analysis, a live request log,
credential health, quota windows, pricing and sessions — with braille graphs,
gradient meters and sparklines.

```text
 cpatop ⌂ localhost:8318 │ admin │ ◷ today │ ⟳ 10s │ ↻ 0s ago │ ● ok                                                                                     ? help
─┤ Overview ├┤ Realtime ├┤ Analysis ├┤ Requests ├┤ Credentials ├┤ Quota ├┤ Models ├┤ Keys ├─────────────────────────────────────────────────── layout balanced ─
╭─¹ totals ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────── today ─╮
│ requests 1,656   ✓ 1,615  ✗ 41   success ■■■■■■■■■■■■ 97.52%   tokens 198M   in 196M   out 1.42M   cache r 187M   cache w 8.34M   reasoning 458k             │
│ cost $126.88   rpm 1.57   tpm 187k   cache ■■■■■■■■■■■■ 95.4%                                                                                                │
│                                                                                                                                                              │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
╭─² trend ───────────────────────────────────────────────────────────── peak 76.3M/h · Σ 198M ─╮╭─³ activity ──────────────────────────────────── day · short ─╮
│ tokens                                                                                       ││ health 97.5%  ✓1,614 ✗41  tokens 198M                        │
│                                                                 ⢀⣷                           ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│                                                                 ⣾⣿⡄                          ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│                                                                ⣸⣿⣿⣇                          ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│                                                               ⢰⣿⣿⣿⣿                          ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│                                                              ⢀⣿⣿⣿⣿⣿⡇                         ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│                                                             ⣠⣾⣿⣿⣿⣿⣿⣷                         ││ ■■■■■■■                                                      │
│                                                           ⢀⣴⣿⣿⣿⣿⣿⣿⣿⣿⡄                        ││                                                              │
│                                                          ⣠⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣇                        ││ tokens                                                       │
│               ⣠⡀                                        ⣴⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿                        ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│             ⢀⣴⣿⣿⣦⡀                             ⢀⣠⣤⣤⡀  ⢀⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣧⣀                      ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│        ⣀⣀⣀⣀⣠⣿⣿⣿⣿⣿⣿⣦⣤⣀⣀⡀               ⣀⣀⣀⣤⣤⣴⣶⣶⣿⣿⣿⣿⣿⣿⣷⣦⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣷⣄⡀                   ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│ requests peak 663 · Σ 1,656                                                                  ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│                                                                 ⢠⣷                           ││ ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■         │
│                                                                ⢠⣿⣿⡆                          ││ ■■■■■■■                                                      │
│                                                               ⢠⣿⣿⣿⣷                          ││                                                              │
│                                                             ⢀⣠⣿⣿⣿⣿⣿⡆                         ││                                                              │
│                                                          ⢀⣤⣾⣿⣿⣿⣿⣿⣿⣿⣷                         ││                                                              │
│                                                         ⣰⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡆                        ││                                                              │
│              ⣠⣴⣦⡀                                     ⢀⣼⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣷                        ││                                                              │
│        ⣀⣀⣀⣀⣠⣾⣿⣿⣿⣿⣦⣄⣀⣀⣀⡀               ⣀⣀⣀⣀⣀⣀⣀⣀⣀⣤⣴⣶⣶⣶⣦⣤⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣦⣤⣀⣀⡀                   ││                                                              │
╰─ 11-12 23:30 ────────────────────────────────────────────────────────────────── 11-13 23:30 ─╯╰─ 11-13 00:00 ────────────────────────────────── 11-14 00:00 ─╯
╭─⁴ distribution ◆ ──────────────────────────────────────────────────────────────────────────────────────────────────── models│api keys│credentials│providers ─╮
│name                                                                                               tokens  share     req  fail      cost  cache trend         │
│claude-opus-5-5                                                                                      139M  70.3%   1,137     4    $73.76  95.9% ▁▁▃▁▁▁▁▂▂▁█▁▁▁│
│claude-fable-5-2                                                                                    45.1M  22.8%     315    37    $47.63  94.3% ▁▁▁▁▁▁▁▁▅█▂▁▁▁│
│gpt-6.1-sol                                                                                         6.41M   3.2%      73     0     $1.52  96.9% ▁▁▁▁▁▁▁▁▁█▁▁▁▁│
│gpt-6-luna                                                                                          6.15M   3.1%      95     0   $0.1338  93.0% ▁▁▁▁▁▁▁▁▁█▂▁▁▁│
│gpt-6-astra                                                                                          961k   0.5%      33     0     $2.33  89.3% ▁▁▁▁▁▁▁▁▁▂█▁▁▁│
│claude-fable-5                                                                                       161k   0.1%       3     0     $1.50  42.0% ▁▁▁▁▁▁▁▁▁▁█▁▁▁│
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

<sub>Captured from a live Keeper at 160×48 with `--privacy`; colours are not shown here.</sub>

## Features

- **Both Keeper roles.** An *admin* session gets every tab (Overview, Realtime,
  Analysis, Requests, Credentials, Quota, Models, Keys); a *CPA API key* session
  gets the per-key dashboard (Overview, Realtime, Analysis). The UI adapts to
  the role the server reports.
- **Live request log** over Server-Sent Events, with reconnect/backoff,
  `Retry-After` handling, `reset` frames and an idle watchdog; scrolling to the
  bottom pages in older requests.
- **btop look:** rounded panes with `─¹ name ─` titles, braille/block/ascii
  graphs with a low→high colour gradient, `■` meters, per-row sparklines,
  right-aligned numbers, heatmaps.
- **Responsive:** layouts degrade from 220×60 down to 80×24 by dropping
  low-priority columns and panes.
- **Never blocks on the network:** each data group is polled by its own task on
  its own cadence; when a poll fails the last good data stays on screen and
  the header shows a stale marker.
- **Themes:** `default` (btop-like), `light`, `gruvbox`, `nord`, `tokyo-night`,
  `dracula`, `monochrome`, plus your own TOML themes.
- Persists theme, range, layouts, hidden panes, sorting and the last tab.

## Install

Requires Rust 1.88+.

```sh
cargo install --path .           # or: cargo build --release
```

TLS uses rustls (no OpenSSL), so a static Linux binary builds with:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
# without musl-gcc installed, point ring's C build at your compiler:
#   CC_x86_64_unknown_linux_musl=gcc cargo build --release --target x86_64-unknown-linux-musl
```

OS-keyring support (macOS Keychain, freedesktop Secret Service) is on by
default; build with `--no-default-features` to leave it out.

## Usage

```sh
# Admin dashboard, password read from a file
cpatop --password-file ~/.config/cpatop/admin.pw

# Per-key dashboard
cpatop --api-key-file ~/.config/cpatop/cpa.key

# Different server, including a sub-path deployment
cpatop --base-url https://host.example/keeper
CPATOP_BASE_URL=https://host.example/keeper cpatop

# No credential configured: an in-app login screen (Admin / API key) appears
cpatop
```

Other commands:

| Command | Purpose |
|---|---|
| `cpatop keyring set admin` / `api-key` | Store a secret in the OS keyring (hidden prompt) |
| `cpatop keyring delete admin` / `api-key` | Remove it |
| `cpatop logout` | Log out on the server and forget the stored session |
| `cpatop themes [--dump NAME]` | List themes / print one as a TOML starting point |
| `cpatop paths` | Show the config, theme and state paths in use |

Useful flags: `--range 7d`, `--refresh 5`, `--theme nord`, `--symbols block`,
`--auth api-key` (prefer the key when both credentials exist), `--privacy`
(mask e-mail addresses), `--no-mouse`, `--no-keyring`, `--fresh-session`.
Run `cpatop --help` for the full list.

## Keys

| Key | Action |
|---|---|
| `Tab` / `→`, `Shift-Tab` / `←` | Next / previous tab (or click a tab) |
| `1`–`9` | Toggle the pane with that number |
| `p` / `l`, `P` | Next / previous layout preset |
| `f` | Focus the next table (or click a pane) |
| `↑↓` `j`/`k`, `PgUp`/`PgDn`, `g`/`G` | Move the selection (mouse wheel scrolls) |
| `/` | Filter the focused table (`Enter` keeps, `Esc` clears) |
| `s` / `S` | Sort by the next column / flip direction |
| `d` | Switch dimension: models, API keys, credentials, providers |
| `[` `]`, `r` | Previous / next time range (today, yesterday, 5h … 30d) |
| `w` | Realtime window 15m / 30m / 60m |
| `a` | Cycle the API-key filter (admin) |
| `e` | Request log: all / successes / failures |
| `+` / `-` | Slower / faster refresh |
| `R` | Refresh now |
| `t` / `T` | Next / previous theme |
| `b` | Graph symbols: braille / block / ascii |
| `x` | Privacy mode |
| `X` `X` | Log out |
| `?` | Help; `q` or `Esc` quits |

## Configuration

`$XDG_CONFIG_HOME/cpatop/config.toml` (see [`config.example.toml`](config.example.toml)).
Precedence: command line and environment → saved UI state → config file → defaults.

| File | Location |
|---|---|
| Config | `$XDG_CONFIG_HOME/cpatop/config.toml` |
| UI state (written automatically) | `$XDG_CONFIG_HOME/cpatop/ui-state.toml` |
| User themes | `$XDG_CONFIG_HOME/cpatop/themes/*.toml` |
| Session cookie (mode 0600) | `$XDG_STATE_HOME/cpatop/session.json` |

On macOS without `XDG_*` variables the platform directories are used
(`cpatop paths` prints them).

Polling defaults follow the web app: overview/realtime every 10 s,
credentials/quota/keys every 60 s, analysis and models on entry, range change
or `R`.

## Themes

A theme is a flat TOML file; every key is optional and falls back to the
`default` theme. Colours are `#rrggbb`, `#rgb`, an ANSI name (`red`,
`darkgray`, …) or `default` for the terminal's colour.

```toml
# ~/.config/cpatop/themes/mine.toml  →  selectable as "mine"
name = "mine"
main_bg = "default"
main_fg = "#cccccc"
title = "#eeeeee"
hi_fg = "#b54040"         # pane numbers, highlights
border = "#3d7b46"
border_focus = "#50f095"
selected_bg = "#6a2f2f"
selected_fg = "#eeeeee"
inactive_fg = "#606060"
graph_text = "#606060"
meter_bg = "#303030"      # empty meter cells
gradient_low = "#50f095"  # graph/meter gradient, bottom → top
gradient_mid = "#f2e266"
gradient_high = "#fa1e1e"
ok = "#50f095"
warn = "#f2e266"
err = "#fa4040"
accent = "#4897d4"
```

`cpatop themes --dump nord > ~/.config/cpatop/themes/my-nord.toml` gives you a
complete starting point. A user theme with a built-in name replaces it.

## Security

- **No secrets on the command line.** Credentials come from
  `--password-file`/`--api-key-file` (or the same keys in the config file),
  `CPATOP_PASSWORD`/`CPATOP_API_KEY`, the OS keyring, or the login screen —
  in that order. cpatop warns when a secret file is readable by others.
- **Only the session cookie is persisted**, never the password or API key, in a
  0600 file in the state directory. It is reused until the server answers 401;
  then cpatop logs in again if a credential source is available, otherwise it
  shows the login screen. A secret typed into the login screen is kept in
  memory only.
- Secrets are never logged or rendered (the login field shows bullets).
  API keys returned by the server are shown only as a short prefix (`abc…`);
  the raw-key endpoint `/usage/api-keys/settings` is never called.
- cpatop only issues GET requests, plus `POST /auth/login`,
  `/auth/api-key-login` and `/auth/logout`. It never changes Keeper settings
  or data, never triggers quota refreshes and never calls the GitHub-backed
  update check.
- `--privacy` (or `x`) masks e-mail addresses, handy for screenshots.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests parse every recorded API response in `tests/fixtures/` (scrubbed real
Keeper v2.1.0 responses) and snapshot the main screens with ratatui's
`TestBackend` at 80×24 and 160×48 for both roles (`tests/snapshots/`, via
[insta](https://insta.rs)).

## License

MIT — see [LICENSE](LICENSE).
