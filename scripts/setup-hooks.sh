#!/usr/bin/env bash
# Opt in to the local CI git hooks for this clone and check prerequisites.
# Optional: GitHub Actions runs CI on every push to main and every pull
# request, and builds releases from v* tags; the hooks only add a local
# pre-flight (.githooks/post-commit runs scripts/ci.sh on main,
# .githooks/pre-push checks its result before pushing main).
#
#   scripts/setup-hooks.sh          set core.hooksPath=.githooks, check tools
#   scripts/setup-hooks.sh --mac    also install Rust (rustup, user-level) on
#                                   the Mac if it is missing
#
# Idempotent. core.hooksPath is per repository (shared by its worktrees).
# Opt out again with: git config --unset core.hooksPath

set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel) || exit 1
cd "$root" || exit 1
# shellcheck source=scripts/lib.sh
. "$here/lib.sh"

mac=0
case "${1:-}" in
    --mac) mac=1 ;;
    "") ;;
    *) sed -n '2,/^$/s/^# \{0,1\}//p' "$0" >&2; exit 2 ;;
esac

current=$(git config --get core.hooksPath || true)
if [ "$current" = .githooks ]; then
    echo "hooks: already enabled (core.hooksPath=.githooks)"
else
    [ -n "$current" ] && echo "hooks: replacing core.hooksPath=$current"
    git config core.hooksPath .githooks
    echo "hooks: enabled (core.hooksPath=.githooks)"
fi
chmod +x .githooks/* scripts/*.sh 2>/dev/null

missing=0
check() { # check <required|optional> <command> <what it is for / how to get it>
    if command -v "$2" >/dev/null 2>&1; then
        printf '  ok        %s\n' "$2"
    else
        printf '  %-9s %s — %s\n' "$([ "$1" = required ] && echo MISSING || echo missing)" "$2" "$3"
        [ "$1" = required ] && missing=1
    fi
}
echo "local tools:"
check required git "git"
check required cargo "Rust toolchain (https://rustup.rs)"
check required flock "util-linux"
check required ssh "openssh (Mac builds)"
check optional cargo-zigbuild "static musl builds: uv tool install cargo-zigbuild (brings zig)"
check optional shellcheck "script lint: uv tool install shellcheck-py"
check optional actionlint "workflow lint: uv tool install actionlint-py"
check optional notify-send "desktop notifications (libnotify; gdbus is the fallback)"
check optional gh "release.sh publishing: GitHub CLI"
if command -v cargo-zigbuild >/dev/null 2>&1 && ! zig_env; then
    printf '  missing   zig — cargo-zigbuild needs it (pip/uv: ziglang)\n'
fi
if command -v rustup >/dev/null 2>&1; then
    for t in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
        if rustup target list --installed | grep -qx "$t"; then
            printf '  ok        rust target %s\n' "$t"
        else
            printf '  missing   rust target %s — rustup target add %s\n' "$t" "$t"
        fi
    done
fi

echo "mac ($CPATOP_MAC_HOST):"
if ! mac_reachable; then
    echo "  unreachable over ssh (BatchMode) — ci.sh --mac skips it, release.sh needs it"
elif mac_has_rust; then
    echo "  ok        reachable, Rust installed"
elif [ "$mac" = 1 ]; then
    echo "  installing rustup (user-level, minimal profile)..."
    # shellcheck disable=SC2016 # expands on the Mac
    mac_ssh 'set -e
        curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path
        "$HOME/.cargo/bin/rustup" component add clippy rustfmt
        "$HOME/.cargo/bin/rustup" target add x86_64-apple-darwin aarch64-apple-darwin' ||
        echo "  rustup install failed"
else
    echo "  reachable, but no Rust — run scripts/setup-hooks.sh --mac"
fi

exit "$missing"
