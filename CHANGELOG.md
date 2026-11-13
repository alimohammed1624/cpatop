# Changelog

All notable changes to cpatop are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Update notice: at most once a day, in the background, cpatop asks GitHub
  for its own latest release (only a `cpatop/<version>` User-Agent is sent;
  cached in the state directory). A newer stable release shows `↑ vX.Y.Z`
  in the header and the upgrade command in `?` help. `cpatop check-update`
  checks now. Off with `--no-update-check`, `update_check = false` or
  `CPATOP_NO_UPDATE_CHECK=1`, and skipped when `CI` is set.
- `Enter` on any table opens every field of the selected row, untruncated;
  `↑↓` move the selection underneath.
- The Overview distribution charts the selected row's tokens in its spare
  rows (like the API-key table).
- One-line installer: `curl -fsSL https://raw.githubusercontent.com/alimohammed1624/cpatop/main/install.sh | sh`
  downloads the prebuilt binary for the machine, checks it against
  `SHA256SUMS` and installs to `~/.local/bin` (`CPATOP_INSTALL_DIR`,
  `CPATOP_VERSION`). Release archives are named without the version
  (`cpatop-<target>.tar.gz`) so `releases/latest/download/` always works.
- `--logout-on-exit` (config `logout_on_exit`) ends the server session on
  quit; by default `q` keeps it and the next start reuses it.
- The help overlay scrolls (`↑↓` `j`/`k` `PgUp`/`PgDn` `g`/`G`, wheel)
  with a position indicator and uses up to three columns on wide
  terminals; only `Esc`, `?` and `q` close it.
- Spare space shows more detail: the selected API key's token trend, tokens
  per quota window, a 5 h requests/failures chart in the credential
  detail; sparkline columns widen with the table.
- The header shows the refresh cadence of the tab on screen (`10s`, `1m`,
  `live`, `on R`); stale data is marked amber in the header and `◌ stale`
  on panes.
- Credentials that share a name get a provider suffix (`· codex`)
  everywhere credentials are listed; the selected row's full name is shown
  on the focused table's border when its name is cut.
- Request log filters (result `e`, model `m`, source `o`, clear `c`) are
  sent to the server on the live stream and on history pages, so totals and
  paging match the filter; model and source pickers come from
  `/usage/events/filters/*`.
- Searchable pickers for the API-key filter (`a`) and the time range (`r`),
  including custom day ranges (up to 365 days, 90 on Requests) and custom
  hour ranges (5–24 hours), validated against the server's limits; a `409`
  clamps the range and refetches.
- API keys are named `#id alias prefix…` everywhere so keys that share a
  masked prefix can be told apart.
- Graphs have y-axis labels, time ticks and a cursor: focus a graph with `f`
  and move through buckets with `←`/`→` (or click) to read exact values.
- Activity heatmaps have row time labels and a colour legend.
- The focused pane lists its keys in its bottom border; `X` shows a
  "press X again to log out (3s)" prompt.
- Release workflow building Linux (x86_64, aarch64; static musl) and macOS
  (x86_64, aarch64, universal) binaries.
- Key-event tests and HTTP integration tests against a mock Keeper.
- The release workflow runs `cargo fmt --check`, clippy and the tests
  before building, refuses a tag that is not `v` + the `Cargo.toml`
  version, and treats a manual run on a branch as a dry run (builds, never
  publishes).
- The README lists every release archive, including the universal macOS
  binary (`cpatop-universal-apple-darwin.tar.gz`) for manual download.

### Changed
- Privacy mode is enforced at the rendering boundary: account names,
  sources, key aliases, endpoints and user agents are replaced (not
  pattern-scrubbed); the base URL shows only scheme, host and port; free
  text masks e-mails anywhere (URL paths included), every IPv4/IPv6 in any
  case or context, cut-off addresses and opaque tokens of 20+ characters;
  and every rendered frame is scanned and masked as a last resort.
  `account-N` now comes from the server's credential id, so labels are
  stable across runs; key aliases are hidden.
- Names in tables are never cut while optional columns can make room
  (`account-12 · codex` no longer reads `accoun… · codex`).
- Keys: name and key columns hidden in every row (privacy mode, no
  aliases) are dropped.
- Token, request and latency-density graphs use the single-hue volume
  ramp; green/amber/red stay reserved for rates and health.
- The graph cursor highlights its bucket's whole column in reverse video.
- Large terminals: graph panes stop growing at a readable height, rows go
  to tables and the charts under them; the activity pane asks only for the
  rows its maps use; message-only panes (quota history without data) stay
  one line tall.
- Both activity maps always carry a title, also when stacked at 80×24.
- Analysis series are rebuilt on dense, calendar-aligned axes (UTC hours
  or local days) with zeros for missing buckets, clipped to now; averages
  and rates divide by the span the data covers. Analysis refreshes every
  60 s.
- Activity heatmaps use whole-hour (or whole-day) rows in local time and
  choose span, cell width and side-by-side layout to fill the pane; titles
  no longer overwrite row labels and the legend wraps instead of being cut.
- API keys show the server's masked `prefix…suffix` like the web UI
  (privacy mode still shows only `key #id`).
- Volumes (token heat, share meters, key × model heatmap) use a single-hue
  ramp; cache and success rates are coloured good/bad; muted text is
  brighter in every theme; credentials without requests show a neutral
  track.
- Names in tables are truncated in the middle (`claude-…-5-2`); Quota
  keeps plan and status (shrinking to `⚠`) at 80×24.
- The graph cursor starts on the last bucket with data; `today` axes stop
  at the current hour.
- The API-key role no longer shows the one-row key × model heatmap or an
  "api keys" dimension.
- `Esc` never quits: it closes overlays, clears filters and graph cursors;
  quit with `q` or Ctrl-C. `r` opens the range picker (`[`/`]` still cycle).
- `--privacy` hides every e-mail address (`account-N`), key fragment
  (`key #id`) and IP address, including in error text.
- Tables size columns to their content: names are no longer stretched,
  spare width goes to truncated columns first, then to meters.
- Panes size to their content; on small terminals low-priority panes
  collapse to one-line summaries instead of being squeezed.
- The Keys tab fetches its own range-scoped usage and shows its freshness.
- Sessions are stored per base URL and role; an explicit `--password-file`,
  `--api-key-file` or environment credential decides the role.
- Analysis composition starts on credentials (models have their own pane);
  the heatmap ranks model columns by tokens.
- Without colour (`NO_COLOR`) meters, heat cells and selections stay
  readable (distinct glyphs, reverse video).

### Fixed
- Table sparklines top out at `▇` so bars in neighbouring rows no longer
  merge into one tall block; the screenshot renderer clips block glyphs
  per cell. The graph cursor reads `11-13 00:00` (not `11-13 00`).
- `today`/`yesterday` hourly series in half-hour zones start at local
  midnight: the first UTC-hour bucket (only data after midnight) is
  labelled `00:00` instead of `23:30` of the previous day.
- Quota's requests chart shows all credentials (titled as such) when the
  selected one has no requests in the health window, and is left out when
  none has.
- Realtime share and speed tables share a row sized to their rows.
- The Models recalc status is derived from status and stage
  (`completed`, `running · recalculating events · 42%`).
- The activity title names the server's fixed grid span (`last 30d`).
- Privacy mode keeps a user agent's product/version tokens.
- Without colour the active tab is bracketed (`[Quota]`); some terminals
  drop bold and reverse along with colour.
- Quota charts the selected credential's recent requests under the table;
  the Keys stacked layout gives the key chart the full width.
- Realtime trends are labelled (`trend ↓12%`, second half vs first) and
  neutral in colour; pricing headers are spelled out when there is room.
- Without colour the realtime outcomes legend (`█ ok ░ fail`) moves to the
  plot when the head line is full instead of being cut at 80×24.
- Analysis averages and rpm/tpm divided by the nominal range length
  (Σ/30 for `30d`) although the server's range is calendar aligned
  (Oct 14 00:00 → Nov 14 00:00); they now divide by the returned range
  clipped to now and say so: `avg/day 12.6M over 30.8d (Oct 14 → now)`.
- Realtime trends no longer explode when the first half of the window is
  nearly empty (`↑8800.0%`): with fewer than 5 requests before the midpoint
  no trend is shown, and larger changes read `↑>999%`. Model speed halves
  without samples read `no samples` instead of `-`.
- Hourly graphs in zones with a half-hour offset (+05:30) labelled their
  x axis `23:30 04:30 09:30`; ticks now sit on whole local hours (`00:00
  06:00 12:00`), placed inside the UTC-hour buckets.
- `Esc` pressed quickly before another key (which terminals send as
  Alt+key) no longer leaves a picker open with the key typed into its
  filter; it closes the picker (or steps back from the custom-range editor)
  and the key takes effect.
- With `--logout-on-exit` the help overlay said quitting keeps the server
  session; it now says `quit and log out on the server`.
- Wording: counts are pluralised (`1 key × 3 models`, `1 row`); the
  outcomes header says `fail rate`; pricing rules read `0 branches ·
  0 conditions` instead of `0b · 0c`; the credentials priority column is
  titled `prio` when the table has room; the Keys server pane shows the
  poller's last pull in plain words (`last pull  no new usage`).
- Quota status contradicted its bars (`⚠ limit reached` next to 5h 93% /
  Weekly 77% left, a 78%-left bar in red): bars are coloured by what is
  left (red only below 5%), a limit is reached only where a window has
  about nothing left and the `⚠` sits on that window, and a server limit
  flag the windows do not confirm reads `◌ limit flag (cached 3h ago)`.
- `--privacy` now also hides IP addresses in the server address (header,
  Keys server pane, login and offline screens) and inside URLs, catches
  lowercase opaque keys (`sk-…`) and IP-valued aliases; `--help` and the
  help overlay describe it correctly.
- Without colour, failed requests in outcome bars are drawn as `░`, and the
  activity legend shows the glyphs the maps use.
- Money, compact-number, rate and duration formatting carried incorrectly
  at rounding boundaries (`$1.999` → `$2.00`, `999.96k` → `1.00M`).
- Sparklines no longer interpolate between buckets; missing buckets render
  as gaps.
- The login screen shows warnings and wrapped errors inside its box.
- `--fresh-session` logs the stored sessions out on the server before
  forgetting them, instead of leaving them valid until they expire.
- Command-line errors are one uncoloured `cpatop: …` line on stderr, with a
  `hint:` where it helps; bad flag values, base URLs, missing config or
  credential files and unknown themes exit with 2, runtime failures with 1.
  Starting the dashboard without a terminal reports that instead of
  panicking. Set `RUST_BACKTRACE=1` for the full error report.

## [0.1.0] - 2026-11-13

First version: admin and API-key dashboards (Overview, Realtime, Analysis,
Requests, Credentials, Quota, Models, Keys), live request log over SSE,
themes, layouts and persisted UI state.
