# Changelog

All notable changes to cpatop are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
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
- Money, compact-number, rate and duration formatting carried incorrectly
  at rounding boundaries (`$1.999` → `$2.00`, `999.96k` → `1.00M`).
- Sparklines no longer interpolate between buckets; missing buckets render
  as gaps.
- The login screen shows warnings and wrapped errors inside its box.

## [0.1.0] - 2026-11-13

First version: admin and API-key dashboards (Overview, Realtime, Analysis,
Requests, Credentials, Quota, Models, Keys), live request log over SSE,
themes, layouts and persisted UI state.
