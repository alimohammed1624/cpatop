# shellcheck shell=bash
# Shared helpers for scripts/ci.sh, scripts/release.sh and .githooks/*.
# Sourced, never run directly. Needs git, flock (util-linux) and ssh.
#
# Local CI state lives in the git dir (never committed), one set per commit:
#   <git-common-dir>/cpatop-ci/<sha>.log      full log of the last run
#   <git-common-dir>/cpatop-ci/<sha>.status   PASS | FAIL | RUNNING, then details
#   <git-common-dir>/cpatop-ci/<sha>.lock     held (flock) while a run is going
#   <git-common-dir>/cpatop-ci/<sha>.unseen   result not yet shown to anyone
#   <git-common-dir>/cpatop-ci/target/        cargo target dir for CI builds
#
# The Mac (CPATOP_MAC_HOST, default "mba") gets the exact commit via
# `git archive | ssh`, never via its own network access to GitHub. Remote
# layout: ~/.cache/cpatop-ci/src/<sha>-<id>/ (pruned) and a persistent
# ~/.cache/cpatop-ci/target/.

CPATOP_MAC_HOST="${CPATOP_MAC_HOST:-mba}"

# Directory holding CI logs/status for this clone (shared by its worktrees).
ci_dir() {
    local common
    common=$(git rev-parse --path-format=absolute --git-common-dir) || return 1
    printf '%s/cpatop-ci\n' "$common"
}

# ci_state <sha>: prints PASS, FAIL, RUNNING or NONE. A RUNNING status whose
# lock is no longer held (the run was killed) counts as NONE.
ci_state() {
    local dir sha=$1
    dir=$(ci_dir) || return 1
    if [ -e "$dir/$sha.lock" ] && ! flock -n "$dir/$sha.lock" true 2>/dev/null; then
        echo RUNNING
        return
    fi
    if [ -f "$dir/$sha.status" ]; then
        case "$(head -n1 "$dir/$sha.status")" in
            PASS*) echo PASS ;;
            FAIL*) echo FAIL ;;
            *) echo NONE ;;
        esac
    else
        echo NONE
    fi
}

# ci_mode <sha>: "full" or "quick" for the recorded run (empty if none).
ci_mode() {
    local dir
    dir=$(ci_dir) || return 1
    sed -n 's/^mode: //p' "$dir/$1.status" 2>/dev/null | head -n1
}

# ci_wait <sha>: block until no CI run holds the lock for <sha>.
ci_wait() {
    local dir
    dir=$(ci_dir) || return 1
    [ -e "$dir/$1.lock" ] && flock "$dir/$1.lock" true 2>/dev/null
    return 0
}

# ci_announce: print one line per finished run nobody has been told about
# (no desktop notification, or a FAIL) and mark it seen.
ci_announce() {
    local dir f sha line
    dir=$(ci_dir) || return 0
    for f in "$dir"/*.unseen; do
        [ -e "$f" ] || continue
        sha=$(basename "$f" .unseen)
        line=$(head -n1 "$dir/$sha.status" 2>/dev/null) || line=""
        case "$line" in
            PASS* | FAIL*)
                printf 'cpatop CI: %s (log: %s)\n' "$line" "$dir/$sha.log" >&2
                rm -f "$f"
                ;;
        esac
    done
}

# ci_snapshot <sha> <dir>: extract the committed tree of <sha> into <dir>.
# Files get the current time (tar -m), not the commit time: cargo decides
# freshness by mtime, and older mtimes would let it reuse stale builds (with
# the previous snapshot's CARGO_MANIFEST_DIR baked into the tests).
ci_snapshot() {
    rm -rf "$2"
    mkdir -p "$2"
    git archive --format=tar "$1" | tar -x -m -C "$2"
}

mac_ssh() {
    ssh -o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=30 \
        "$CPATOP_MAC_HOST" "$@"
}

mac_reachable() {
    mac_ssh true >/dev/null 2>&1
}

mac_has_rust() {
    # shellcheck disable=SC2016 # $HOME expands on the Mac
    mac_ssh 'test -x "$HOME/.cargo/bin/cargo"' >/dev/null 2>&1
}

# mac_ship <sha> <id>: copy the tree of <sha> to the Mac and print the remote
# directory (relative to the Mac's $HOME). Keeps the 5 newest source dirs.
mac_ship() {
    local rdir=".cache/cpatop-ci/src/$1-$2"
    git archive --format=tar "$1" | mac_ssh "set -e
        d=\"\$HOME/$rdir\"
        rm -rf \"\$d\" && mkdir -p \"\$d\" && tar -x -m -C \"\$d\"
        cd \"\$HOME/.cache/cpatop-ci/src\"
        ls -1t | tail -n +6 | while IFS= read -r old; do rm -rf \"./\$old\"; done" || return 1
    printf '%s\n' "$rdir"
}

# mac_run <remote dir> <shell commands>: run bash on the Mac inside <remote
# dir> with rustup's cargo on PATH and the persistent target dir.
mac_run() {
    local rdir=$1
    shift
    # The body sits in a function so bash has read all of it before any
    # command runs; commands get /dev/null as stdin, not the script.
    mac_ssh 'bash -s' <<EOF
main() {
set -euo pipefail
export PATH="\$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_TARGET_DIR="\$HOME/.cache/cpatop-ci/target" CARGO_TERM_COLOR=never CI=true
cd "\$HOME/$rdir"
$*
}
main </dev/null
EOF
}

# mac_cleanup <remote dir>
mac_cleanup() {
    mac_ssh "rm -rf \"\$HOME/$1\"" >/dev/null 2>&1 || true
}

# fmt_secs <seconds>: 75 -> 1m15s
fmt_secs() {
    if [ "$1" -ge 60 ]; then
        printf '%dm%02ds' $(($1 / 60)) $(($1 % 60))
    else
        printf '%ds' "$1"
    fi
}

# zig_env: make cargo-zigbuild find zig (a `zig` on PATH, or the ziglang
# Python package that `uv tool install cargo-zigbuild` / pip installs).
zig_env() {
    command -v cargo-zigbuild >/dev/null 2>&1 || return 1
    command -v zig >/dev/null 2>&1 && return 0
    [ -n "${CARGO_ZIGBUILD_ZIG_PATH:-}" ] && return 0
    local z
    for z in "$HOME"/.local/share/uv/tools/cargo-zigbuild/lib/python3*/site-packages/ziglang/zig; do
        if [ -x "$z" ]; then
            export CARGO_ZIGBUILD_ZIG_PATH="$z"
            return 0
        fi
    done
    python3 -c 'import ziglang' >/dev/null 2>&1
}

# musl_build <target>: static Linux release build of the current directory.
musl_build() {
    if zig_env; then
        cargo zigbuild --release --locked --target "$1"
    elif [ "$1" = x86_64-unknown-linux-musl ] && command -v musl-gcc >/dev/null 2>&1; then
        cargo build --release --locked --target "$1"
    else
        echo "error: building $1 needs cargo-zigbuild + zig (uv tool install cargo-zigbuild)" >&2
        return 1
    fi
}
