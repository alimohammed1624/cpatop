#!/usr/bin/env bash
# Local CI for cpatop: what .github/workflows/ci.yml checks, run on this
# machine (and optionally on the Mac over ssh) instead of GitHub Actions.
#
#   scripts/ci.sh [--mac] [--quick] [--sha <rev>]
#
#   --mac      also run fmt, clippy and tests on the Mac (CPATOP_MAC_HOST,
#              default "mba"); "mac: skipped (unreachable)" if it is offline
#   --quick    fmt, clippy and tests only: skip the release builds and the
#              script linters
#   --sha REV  commit to test (default HEAD)
#   --notify   desktop notification when done (used by the post-commit hook)
#
# Always tests the committed tree of the commit (via git archive), never
# uncommitted changes. Steps: cargo fmt --check, cargo clippy --all-targets
# -D warnings, cargo test, a static x86_64-unknown-linux-musl release build
# (cargo-zigbuild), shellcheck and actionlint (when installed); with --mac
# also fmt, clippy, test and an aarch64-apple-darwin release build there.
#
# Log and result: <git-common-dir>/cpatop-ci/<sha>.log and <sha>.status
# (first line PASS or FAIL). Exit status 0 = PASS, 1 = FAIL, 2 = usage or
# already running for that commit.

set -uo pipefail

usage() {
    sed -n '2,/^$/s/^# \{0,1\}//p' "$0" >&2
    exit 2
}

main() {
    local mac=0 quick=0 notify=0 rev=HEAD
    while [ $# -gt 0 ]; do
        case "$1" in
            --mac) mac=1 ;;
            --quick) quick=1 ;;
            --notify) notify=1 ;;
            --sha) rev=${2:-}; shift ;;
            --sha=*) rev=${1#--sha=} ;;
            -h | --help) usage ;;
            *) echo "ci.sh: unknown option $1" >&2; usage ;;
        esac
        shift
    done

    local here root
    here=$(cd "$(dirname "$0")" && pwd)
    root=$(git -C "$here" rev-parse --show-toplevel) || exit 2
    cd "$root" || exit 2
    # shellcheck source=scripts/lib.sh
    . "$here/lib.sh"

    local sha dir
    sha=$(git rev-parse --verify --quiet "$rev^{commit}") || { echo "ci.sh: unknown commit $rev" >&2; exit 2; }
    dir=$(ci_dir) || exit 2
    mkdir -p "$dir"
    find "$dir" -maxdepth 1 -type f -mtime +30 -delete 2>/dev/null

    log="$dir/$sha.log"
    local status="$dir/$sha.status" lock="$dir/$sha.lock"
    exec 9>"$lock"
    if ! flock -n 9; then
        echo "cpatop CI: already running for ${sha:0:7} (log: $log)" >&2
        exit 2
    fi

    local mode=full
    [ "$quick" = 1 ] && mode=quick
    local subject
    subject=$(git log -1 --format=%s "$sha")
    printf 'RUNNING\nmode: %s\npid: %s\nstarted: %s\n' "$mode" "$$" "$(date -Iseconds)" >"$status"
    rm -f "$dir/$sha.unseen"
    : >"$log"
    say "cpatop CI ${sha:0:7} ($mode) — $subject"
    say "log: $log"
    if [ "$sha" = "$(git rev-parse HEAD)" ] && [ -n "$(git status --porcelain --untracked-files=no 2>/dev/null)" ]; then
        say "note: testing the committed HEAD; uncommitted changes are not included"
    fi

    # A private copy of the commit, so later edits or commits in the work
    # tree cannot change what is being tested.
    src="$dir/src/$sha.$$"
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    ci_snapshot "$sha" "$src" || { echo "ci.sh: could not extract $sha" >&2; exit 2; }
    export CARGO_TARGET_DIR="$dir/target" CARGO_TERM_COLOR=never CI=true

    {
        echo "commit: $sha $subject"
        echo "host: $(uname -srm)"
        cargo --version
        rustc --version
    } >>"$log" 2>&1

    results="$dir/src/$sha.$$.results"
    : >"$results"
    local t0=$SECONDS

    # The Mac runs alongside the local steps; its output goes to a side log.
    local macpid="" maclog="$dir/src/$sha.$$.mac.log"
    if [ "$mac" = 1 ]; then
        # fd 3: progress lines for the terminal.
        mac_steps "$sha" "$quick" 3>&1 >"$maclog" 2>&1 &
        macpid=$!
    fi

    (cd "$src" && step fmt cargo fmt --check)
    (cd "$src" && step clippy cargo clippy --locked --all-targets -- -D warnings)
    (cd "$src" && step test cargo test --locked)
    if [ "$quick" = 0 ]; then
        (cd "$src" && step musl-build musl_build x86_64-unknown-linux-musl)
        if command -v shellcheck >/dev/null 2>&1; then
            (cd "$src" && step shellcheck lint_scripts)
        else
            record shellcheck "skipped (not installed)" ""
        fi
        if command -v actionlint >/dev/null 2>&1; then
            (cd "$src" && step actionlint actionlint .github/workflows/*.yml)
        else
            record actionlint "skipped (not installed)" ""
        fi
    fi

    if [ -n "$macpid" ]; then
        wait "$macpid"
        {
            echo
            echo "################ mac ($CPATOP_MAC_HOST) ################"
            cat "$maclog"
        } >>"$log"
        grep -h '^@@result ' "$maclog" | sed 's/^@@result //' >>"$results"
    fi

    local verdict=PASS
    grep -q $'\tfail\t' "$results" && verdict=FAIL
    local elapsed
    elapsed=$(fmt_secs $((SECONDS - t0)))

    local summary
    summary=$(awk -F'\t' '{ printf "  %-18s %-26s %s\n", $1, $2, $3 }' "$results")
    {
        printf '%s %s %s\n' "$verdict" "${sha:0:7}" "$elapsed"
        printf 'mode: %s\n' "$mode"
        printf 'finished: %s\n' "$(date -Iseconds)"
        printf '%s\n' "$summary"
    } >"$status.tmp" && mv "$status.tmp" "$status"
    {
        echo
        echo "================ summary ================"
        cat "$status"
    } >>"$log"

    say ""
    say "cpatop CI ${sha:0:7}: $verdict in $elapsed"
    say "$summary"
    [ "$verdict" = FAIL ] && say "log: $log"

    if [ "$notify" = 1 ]; then
        local sent=0
        notify "cpatop CI: $verdict (${sha:0:7}, $elapsed)" "$subject"$'\n'"$log" "$verdict" && sent=1
        echo "desktop notification: $([ "$sent" = 1 ] && echo sent || echo unavailable)" >>"$log"
        # A failure is repeated by the next hook even if a notification went out.
        if [ "$sent" = 0 ] || [ "$verdict" = FAIL ]; then
            : >"$dir/$sha.unseen"
        fi
    fi

    [ "$verdict" = PASS ]
}

say() {
    printf '%s\n' "$*"
}

cleanup() {
    [ -n "${src:-}" ] && rm -rf "$src" "$src.results" "$src.mac.log"
    return 0
}

# notify <title> <body> <PASS|FAIL>: desktop notification via notify-send,
# else straight over D-Bus (gdbus). Fails if neither works.
notify() {
    local urgency=normal level=1
    [ "$3" = FAIL ] && urgency=critical level=2
    if command -v notify-send >/dev/null 2>&1 &&
        notify-send -a cpatop-ci -u "$urgency" "$1" "$2" >/dev/null 2>&1; then
        return 0
    fi
    command -v gdbus >/dev/null 2>&1 &&
        gdbus call --session --dest org.freedesktop.Notifications \
            --object-path /org/freedesktop/Notifications \
            --method org.freedesktop.Notifications.Notify \
            cpatop-ci 0 '' "$1" "$2" '[]' "{'urgency': <byte $level>}" -1 >/dev/null 2>&1
}

# record <name> <result> <seconds>
record() {
    local secs=""
    [ -n "$3" ] && secs=$(fmt_secs "$3")
    printf '%s\t%s\t%s\n' "$1" "$2" "$secs" >>"$results"
}

# Lint every shell script in the repo with shellcheck (inside the snapshot).
lint_scripts() {
    local f files=()
    for f in install.sh scripts/*.sh .githooks/*; do
        [ -f "$f" ] && files+=("$f")
    done
    shellcheck -x "${files[@]}"
}

# step <name> <command...>: run one check, log its output, record pass/fail.
step() {
    local name=$1 t0=$SECONDS rc
    shift
    printf '\n==> %s: %s\n' "$name" "$*" >>"$log"
    local cr=""
    [ -t 1 ] && cr=$'\r' && printf '  %-18s ...' "$name"
    "$@" >>"$log" 2>&1
    rc=$?
    local secs=$((SECONDS - t0))
    if [ "$rc" = 0 ]; then
        printf '%s  %-18s ok   (%s)\n' "$cr" "$name" "$(fmt_secs "$secs")"
        record "$name" pass "$secs"
    else
        printf '%s  %-18s FAIL (%s, exit %s)\n' "$cr" "$name" "$(fmt_secs "$secs")" "$rc"
        printf '<== %s failed (exit %s)\n' "$name" "$rc" >>"$log"
        record "$name" fail "$secs"
    fi
    return 0
}

# mac_steps <sha> <quick>: the Mac half of the run. Prints its log on stdout
# and one "@@result <name>\t<result>\t<time>" line per step.
mac_steps() {
    local sha=$1 quick=$2
    res() { printf '@@result %s\t%s\t%s\n' "$1" "$2" "${3:-}"; }
    if ! mac_reachable; then
        echo "mac: $CPATOP_MAC_HOST unreachable over ssh"
        printf '  %-18s skipped (%s unreachable)\n' mac "$CPATOP_MAC_HOST" >&3
        res mac "skipped (unreachable)"
        return
    fi
    if ! mac_has_rust; then
        echo "mac: no ~/.cargo/bin/cargo on $CPATOP_MAC_HOST (run scripts/setup-hooks.sh --mac)"
        res mac fail
        return
    fi
    local r
    if ! r=$(mac_ship "$sha" "ci$$"); then
        echo "mac: could not copy $sha to $CPATOP_MAC_HOST"
        res mac fail
        return
    fi
    mac_run "$r" 'sw_vers -productVersion; uname -m; cargo --version; rustc --version' 2>&1
    local name cmd t0 secs
    local -a steps=(
        "mac-fmt|cargo fmt --check"
        "mac-clippy|cargo clippy --locked --all-targets -- -D warnings"
        "mac-test|cargo test --locked"
    )
    [ "$quick" = 0 ] && steps+=("mac-build|cargo build --release --locked --target aarch64-apple-darwin")
    for s in "${steps[@]}"; do
        name=${s%%|*}
        cmd=${s#*|}
        t0=$SECONDS
        printf '\n==> %s: %s\n' "$name" "$cmd"
        if mac_run "$r" "$cmd" 2>&1; then
            secs=$((SECONDS - t0))
            res "$name" pass "$(fmt_secs "$secs")"
            printf '  %-18s ok   (%s)\n' "$name" "$(fmt_secs "$secs")" >&3
        else
            secs=$((SECONDS - t0))
            echo "<== $name failed"
            res "$name" fail "$(fmt_secs "$secs")"
            printf '  %-18s FAIL (%s)\n' "$name" "$(fmt_secs "$secs")" >&3
        fi
    done
    mac_cleanup "$r"
}

main "$@"
