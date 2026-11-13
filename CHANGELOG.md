# Changelog

All notable changes to cpatop are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
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

### Changed
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
- Analysis averages and rpm/tpm divided by the nominal range length
  (Σ/30 for `30d`) although the server's range is calendar aligned
  (Oct 14 00:00 → Nov 14 00:00); they now divide by the returned range
  clipped to now and say so: `avg/day 12.6M over 30.8d (Oct 14 → now)`.
- Realtime trends no longer explode when the first half of the window is
  nearly empty (`↑8800.0%`): with fewer than 5 requests before the midpoint
  no trend is shown, and larger changes read `↑>999%`. Model speed halves
  without samples read `no samples` instead of `-`.
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

## [0.1.0] - 2026-11-13

First version: admin and API-key dashboards (Overview, Realtime, Analysis,
Requests, Credentials, Quota, Models, Keys), live request log over SSE,
themes, layouts and persisted UI state.
