#!/usr/bin/env bash
# Render PNG screenshots of cpatop into docs/screenshots/.
#
# Each scene runs cpatop (always with --privacy) in a detached tmux session at
# a fixed size, waits for its data to load, sends keys to reach the scene,
# captures the pane with colours, converts it to HTML with `aha` and renders
# that with headless Chromium.
#
# Requirements: cargo, tmux, aha, chromium, ImageMagick (`magick`), the
# "MesloLGS Nerd Font Mono" font; oxipng is used when installed.
#
# Configuration (environment):
#   CPATOP_BASE_URL            Keeper URL (default http://127.0.0.1:8318: an IP
#                              address, to show that privacy mode masks it)
#   CPATOP_SHOT_PASSWORD_FILE  admin password file (admin scenes)
#   CPATOP_SHOT_API_KEY_FILE   CPA API key file (API-key scenes)
#   SHOT_ONLY                  optional space-separated scene names to render
#   SHOT_TIMEOUT               seconds to wait for data per scene (default 30)
#
# Scenes whose credential is not configured are skipped. Sessions are logged
# out on the server when the script exits.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUT="$ROOT/docs/screenshots"
BASE_URL=${CPATOP_BASE_URL:-http://127.0.0.1:8318}
PW_FILE=${CPATOP_SHOT_PASSWORD_FILE:-}
KEY_FILE=${CPATOP_SHOT_API_KEY_FILE:-}
ONLY=${SHOT_ONLY:-}
TIMEOUT=${SHOT_TIMEOUT:-30}

FONT_PX=14
CELL_W=$(awk -v f="$FONT_PX" 'BEGIN { printf "%.4f", f * 1233 / 2048 }') # Meslo advance
LINE_PX=17.5   # slightly under the font's 1.26em box so box-drawing lines touch
PAD=18         # px of terminal background around the grid
TITLE_H=28     # px for the window title bar
SCALE=2

for tool in cargo tmux aha chromium magick; do
  command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 1; }
done
[[ $(fc-list : family) == *"MesloLGS Nerd Font Mono"* ]] ||
  echo "warning: MesloLGS Nerd Font Mono not found; glyphs may not line up" >&2

TMP=$(mktemp -d "${TMPDIR:-/tmp}/cpatop-shots.XXXXXX")
PREFIX="cpatop-shot-$$"
STATE_ADMIN="$TMP/state-admin"
STATE_KEY="$TMP/state-key"
mkdir -p "$STATE_ADMIN" "$STATE_KEY" "$OUT"

BIN="$ROOT/target/release/cpatop"

cleanup() {
  local s
  for s in $(tmux list-sessions -F '#{session_name}' 2>/dev/null | grep "^$PREFIX-" || true); do
    tmux kill-session -t "$s" 2>/dev/null || true
  done
  # Log the sessions out on the server, as `X X` in the app would.
  local st
  for st in "$STATE_ADMIN" "$STATE_KEY"; do
    if [[ -x $BIN && -d $st ]]; then
      env -u NO_COLOR XDG_CONFIG_HOME="$TMP/cfg-logout" XDG_STATE_HOME="$st" \
        "$BIN" --base-url "$BASE_URL" --no-keyring logout >/dev/null 2>&1 || true
    fi
  done
  rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

echo "building release binary…"
(cd "$ROOT" && cargo build --release --quiet)

# Background colour for a theme: its main_bg, or a dark terminal for "default".
theme_bg() {
  local bg
  bg=$("$BIN" themes --dump "$1" | sed -n 's/^main_bg = "\(.*\)"/\1/p')
  [[ $bg == \#* ]] && echo "$bg" || echo "#101010"
}

capture() { tmux capture-pane -p -t "$1"; }

# Wait until the pane shows loaded data: header reports "● ok", the "? help"
# hint is back (no flash message), and no pane is still loading.
wait_loaded() {
  local sess=$1 deadline=$((SECONDS + TIMEOUT)) text
  while ((SECONDS < deadline)); do
    text=$(capture "$sess" || true)
    if grep -q "● ok" <<<"$text" && grep -q "? help" <<<"$text" &&
      ! grep -qE "loading…|waiting for the live stream|Sign in|logging in" <<<"$text"; then
      sleep 0.7 # let the next frame land (graphs animate in on first data)
      return 0
    fi
    sleep 0.3
  done
  echo "timed out waiting for data in $sess; capturing anyway" >&2
  capture "$sess" | head -3 >&2
  return 0
}

# shot NAME ROLE COLS ROWS THEME_BG [KEY...]
#   KEY is a tmux key name, or "wait" to wait for data again.
shot() {
  local name=$1 role=$2 cols=$3 rows=$4 bgtheme=$5
  shift 5
  if [[ -n $ONLY && " $ONLY " != *" $name "* ]]; then return 0; fi

  local state cred
  case $role in
    admin)
      [[ -n $PW_FILE ]] || { echo "skip $name: CPATOP_SHOT_PASSWORD_FILE not set" >&2; return 0; }
      state=$STATE_ADMIN cred=(--auth admin --password-file "$PW_FILE") ;;
    key)
      [[ -n $KEY_FILE ]] || { echo "skip $name: CPATOP_SHOT_API_KEY_FILE not set" >&2; return 0; }
      state=$STATE_KEY cred=(--auth api-key --api-key-file "$KEY_FILE") ;;
  esac

  local sess="$PREFIX-$name" cfg="$TMP/cfg-$name"
  mkdir -p "$cfg"
  echo "scene $name (${cols}x$rows)"
  # A fresh config dir per scene so saved UI state (theme, tab) never leaks
  # between scenes; the state dir is shared per role to reuse the session.
  local cmd
  printf -v cmd '%q ' env -u NO_COLOR TERM=tmux-256color COLORTERM=truecolor \
    XDG_CONFIG_HOME="$cfg" XDG_STATE_HOME="$state" \
    "$BIN" --base-url "$BASE_URL" --no-keyring --privacy --theme default "${cred[@]}"
  tmux new-session -d -s "$sess" -x "$cols" -y "$rows" "$cmd"
  tmux set-option -t "$sess" status off >/dev/null
  tmux resize-window -t "$sess" -x "$cols" -y "$rows"
  wait_loaded "$sess"

  local k
  for k in "$@"; do
    if [[ $k == wait ]]; then wait_loaded "$sess"; else
      tmux send-keys -t "$sess" "$k"
      sleep 0.25
    fi
  done
  wait_loaded "$sess"

  local ans="$TMP/$name.ans" html="$TMP/$name.html" raw="$TMP/$name.raw.png"
  tmux capture-pane -p -e -t "$sess" >"$ans"
  # Privacy guard: --privacy must leave no e-mail, masked key or IP address.
  if tmux capture-pane -p -t "$sess" |
    grep -nE '@|\*\*\*|[0-9]{1,3}(\.[0-9]{1,3}){3}|[0-9a-f]{1,4}(:[0-9a-f]{0,4}){3,}' >&2; then
    echo "privacy check failed in scene $name; not writing the PNG" >&2
    exit 1
  fi
  # Panes replace identifiers themselves; a placeholder from the render
  # guard means some text needed the last line of defence.
  if tmux capture-pane -p -t "$sess" | grep -nE '‹(key|email)›' >&2; then
    echo "warning: scene $name shows masked free text (see above)" >&2
  fi

  # Quit like a user would; the server session is logged out in cleanup.
  tmux send-keys -t "$sess" q
  local i
  for i in $(seq 1 30); do
    tmux has-session -t "$sess" 2>/dev/null || break
    sleep 0.1
  done
  tmux kill-session -t "$sess" 2>/dev/null || true

  render "$ans" "$html" "$raw" "$cols" "$rows" "$(theme_bg "$bgtheme")"
  # Trim the transparent page around the window, then optimise.
  # A 256-colour palette is plenty for terminal colours and keeps files small.
  magick "$raw" -trim +repage -strip +dither -colors 256 \
    -define png:compression-level=9 "PNG8:$OUT/$name.png"
  if command -v oxipng >/dev/null; then oxipng -q -o 3 --strip all "$OUT/$name.png"; fi
  echo "  -> docs/screenshots/$name.png ($(du -k "$OUT/$name.png" | cut -f1) KB)"
}

render() {
  local ans=$1 html=$2 png=$3 cols=$4 rows=$5 bg=$6
  local gw gh ww wh BLK_SCALE
  # Full block spans 2088/2048 em in Meslo; scale it to the line height (plus a
  # little overlap so anti-aliased edges do not leave seams between cells).
  BLK_SCALE=$(awk -v l="$LINE_PX" -v f="$FONT_PX" 'BEGIN { printf "%.4f", 1.04 * l / (f * 2088 / 2048) }')
  gw=$(awk -v c="$cols" -v w="$CELL_W" 'BEGIN { printf "%d", c * w + 0.999 }')
  gh=$(awk -v r="$rows" -v l="$LINE_PX" 'BEGIN { printf "%d", r * l + 0.999 }')
  ww=$((gw + 2 * PAD + 40))
  wh=$((gh + 2 * PAD + TITLE_H + 40))
  {
    cat <<EOF
<!doctype html><html><head><meta charset="utf-8"><style>
html, body { margin: 0; background: transparent; }
body { padding: 20px; }
.win { display: inline-block; background: $bg; border-radius: 10px;
  box-shadow: 0 0 0 1px rgba(128,128,128,.35); overflow: hidden; }
.bar { height: ${TITLE_H}px; display: flex; align-items: center; gap: 8px;
  padding: 0 14px; background: rgba(127,127,127,.14); }
.bar i { width: 12px; height: 12px; border-radius: 50%; display: block; }
.bar i:nth-child(1) { background: #ff5f57; } .bar i:nth-child(2) { background: #febc2e; }
.bar i:nth-child(3) { background: #28c840; }
pre { margin: 0; padding: ${PAD}px; width: ${gw}px; height: ${gh}px; color: #cccccc;
  font: ${FONT_PX}px/${LINE_PX}px "MesloLGS Nerd Font Mono", monospace;
  font-variant-ligatures: none; white-space: pre; overflow: hidden; }
pre span { line-height: ${LINE_PX}px; }
/* Meslo's block elements are shorter than a cell (terminals draw their own);
   centre them on the cell and stretch them so bars and fills touch. Each is
   clipped to its own cell, like a terminal does, so stretched partial blocks
   (sparklines ▁▂▃) never spill into the rows above or below. */
.cell { display: inline-block; width: ${CELL_W}px; height: ${LINE_PX}px;
  overflow: hidden; vertical-align: top; line-height: ${LINE_PX}px; }
.blk { display: inline-block; transform: scale(1.03, $BLK_SCALE) translateY(-0.0864em); }
</style></head><body><div class="win"><div class="bar"><i></i><i></i><i></i></div><pre>
EOF
    aha --no-header <"$ans" |
      perl -CSD -pe 's/([\x{2580}-\x{259F}])/<span class="cell"><span class="blk">$1<\/span><\/span>/g'
    echo '</pre></div></body></html>'
  } >"$html"
  chromium --headless --disable-gpu --no-first-run --no-default-browser-check \
    --user-data-dir="$TMP/chromium" --hide-scrollbars --force-device-scale-factor="$SCALE" \
    --default-background-color=00000000 --window-size="$ww,$wh" \
    --screenshot="$png" "file://$html" >/dev/null 2>&1
}

# Scenes. Keys follow the in-app help (`?`): Tab moves to the next tab, F2…F9
# jump to a tab, t cycles themes (default → light → gruvbox → nord →
# tokyo-night → …), f focuses the next table or graph, ←/→ move a graph's
# cursor, r opens the range picker, a the API-key picker, e cycles the
# request-log result filter.
# name                 role  cols rows theme       keys
shot admin-overview      admin 160 48 default
shot admin-realtime      admin 160 48 default     Tab
shot admin-analysis      admin 160 48 default     Tab Tab
shot admin-requests      admin 160 48 default     Tab Tab Tab
shot admin-requests-failed admin 160 48 default   F5 wait e e wait
shot admin-credentials   admin 160 48 default     F6
shot admin-quota         admin 160 48 default     F7
shot admin-keys          admin 160 48 default     F9 wait
shot graph-cursor        admin 160 48 default     f Left Left Left Left Left Left
shot range-picker        admin 160 48 default     r Down Down Down
shot key-picker          admin 160 48 default     a
shot apikey-overview     key   160 48 default
shot theme-light         admin 160 48 light       Tab Tab t
shot theme-tokyo-night   admin 160 48 tokyo-night Tab t t t t
shot help-overlay        admin 160 48 default     '?'
shot wide-220x60         admin 220 60 default
shot compact-80x24       admin 80 24 default      Tab
shot row-detail          admin 80 24 default      F5 wait Enter

echo "done: $OUT"
