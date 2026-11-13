#!/usr/bin/env bash
# Fallback: build and publish a cpatop release from this machine plus a Mac.
# The primary path is GitHub Actions: pushing a vX.Y.Z tag runs
# .github/workflows/release.yml, which builds the same assets and publishes.
# Use this script only when Actions is unavailable. The tag it pushes still
# starts the workflow, which then finds the release already made and leaves
# it alone.
#
#   scripts/release.sh vX.Y.Z [--dry-run]
#
# Checks (a real release refuses on any failure; --dry-run only warns):
#   - on branch main, work tree clean, HEAD equal to <remote>/main
#   - tag == "v" + Cargo.toml version, and the tag does not exist yet
#   - CHANGELOG.md has a "## [X.Y.Z]" section (the release notes)
#   - local CI (scripts/ci.sh) passed a full run for HEAD; if not, a real
#     release runs `scripts/ci.sh --mac` first
# Builds the committed tree of HEAD:
#   x86_64-unknown-linux-musl, aarch64-unknown-linux-musl   here (cargo-zigbuild)
#   aarch64-apple-darwin, x86_64-apple-darwin, universal (lipo)  on the Mac
# and packages dist/cpatop-<target>.tar.gz (cpatop-<target>/{cpatop,README.md,
# LICENSE,CHANGELOG.md,config.example.toml}) plus dist/SHA256SUMS, exactly as
# the release workflow does and install.sh expects. A real release then tags
# (git tag -a), pushes the tag and runs `gh release create` with the notes
# and all assets. --dry-run stops after packaging.
#
# Env: CPATOP_MAC_HOST (default mba), CPATOP_REMOTE (default origin).

set -uo pipefail

TARGETS_LINUX=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)
TARGETS_MAC=(aarch64-apple-darwin x86_64-apple-darwin universal-apple-darwin)
PAYLOAD=(README.md LICENSE CHANGELOG.md config.example.toml)

die() {
    printf 'release: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    sed -n '2,/^$/s/^# \{0,1\}//p' "$0" >&2
    exit 2
}

main() {
    local tag="" dry=0
    while [ $# -gt 0 ]; do
        case "$1" in
            --dry-run) dry=1 ;;
            -h | --help) usage ;;
            v*) [ -z "$tag" ] || usage; tag=$1 ;;
            *) usage ;;
        esac
        shift
    done
    [ -n "$tag" ] || usage
    [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "tag must look like v1.2.3 or v1.2.3-rc.1 (got $tag)"

    local here root
    here=$(cd "$(dirname "$0")" && pwd)
    root=$(git -C "$here" rev-parse --show-toplevel) || exit 1
    cd "$root" || exit 1
    # shellcheck source=scripts/lib.sh
    . "$here/lib.sh"
    local remote=${CPATOP_REMOTE:-origin}
    local version=${tag#v}

    problems=0
    # check <ok?> <message>: a failed check is fatal later unless --dry-run.
    check() {
        if [ "$1" = 0 ]; then
            if [ "$dry" = 1 ]; then
                printf 'release: warning (dry run): %s\n' "$2" >&2
            else
                printf 'release: %s\n' "$2" >&2
            fi
            problems=1
        fi
    }

    local branch sha cargo_version
    branch=$(git symbolic-ref --quiet --short HEAD || echo "(detached)")
    sha=$(git rev-parse HEAD)
    check "$([ "$branch" = main ] && echo 1 || echo 0)" "not on main (on $branch)"
    check "$([ -z "$(git status --porcelain)" ] && echo 1 || echo 0)" "work tree is not clean"
    local remote_main
    remote_main=$(git ls-remote "$remote" refs/heads/main 2>/dev/null | cut -f1)
    if [ -n "$remote_main" ]; then
        check "$([ "$sha" = "$remote_main" ] && echo 1 || echo 0)" \
            "HEAD ${sha:0:7} is not $remote main (${remote_main:0:7}); push main first"
    else
        check 0 "could not read main from $remote"
    fi
    cargo_version=$(git show "$sha:Cargo.toml" | sed -n 's/^version = "\(.*\)"$/\1/p' | head -n1)
    check "$([ "$version" = "$cargo_version" ] && echo 1 || echo 0)" \
        "tag $tag does not match Cargo.toml version $cargo_version (expected v$cargo_version)"
    check "$(git rev-parse -q --verify "refs/tags/$tag" >/dev/null && echo 0 || echo 1)" "tag $tag already exists locally"
    check "$([ -z "$(git ls-remote --tags "$remote" "refs/tags/$tag" 2>/dev/null)" ] && echo 1 || echo 0)" \
        "tag $tag already exists on $remote"

    notes=$(changelog_section "$version")
    if [ -z "$notes" ]; then
        check 0 "CHANGELOG.md has no '## [$version]' section"
        notes=$(changelog_section Unreleased)
    fi

    if [ "$dry" = 0 ]; then
        command -v gh >/dev/null 2>&1 || die "gh (GitHub CLI) is required to publish"
        gh release view "$tag" >/dev/null 2>&1 && die "a GitHub release $tag already exists"
        [ "$problems" = 0 ] || die "refusing to release (see above)"
    fi

    # Local CI: a real release needs a full PASS for this exact commit.
    local state cimode
    state=$(ci_state "$sha")
    [ "$state" = RUNNING ] && { echo "release: waiting for the running local CI of ${sha:0:7}..."; ci_wait "$sha"; state=$(ci_state "$sha"); }
    cimode=$(ci_mode "$sha")
    if [ "$state" != PASS ] || [ "$cimode" != full ]; then
        if [ "$dry" = 1 ]; then
            echo "release: warning (dry run): local CI for ${sha:0:7} is ${state,,}${cimode:+ ($cimode)}, not a full PASS" >&2
        else
            echo "release: local CI for ${sha:0:7} is ${state,,}${cimode:+ ($cimode)}; running scripts/ci.sh --mac"
            "$here/ci.sh" --mac --sha "$sha" || die "local CI failed for ${sha:0:7}; not releasing"
        fi
    else
        echo "release: local CI PASS for ${sha:0:7}"
    fi

    mac_reachable || die "Mac $CPATOP_MAC_HOST is unreachable over ssh (set CPATOP_MAC_HOST); the macOS builds need it"
    mac_has_rust || die "no Rust on $CPATOP_MAC_HOST; run scripts/setup-hooks.sh --mac"

    local cidir dist t0=$SECONDS
    cidir=$(ci_dir)
    dist="$root/dist"
    work="$cidir/release.$$"
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    rm -rf "$dist"
    mkdir -p "$dist" "$work/bin"
    ci_snapshot "$sha" "$work/src" || die "could not extract $sha"
    export CARGO_TARGET_DIR="$cidir/target"
    echo "release: building ${sha:0:7} as $tag (logs: $work/*.log until the end, then dist/build-*.log)"

    # The Mac builds while the Linux builds run here.
    mac_build "$sha" >"$work/mac.log" 2>&1 &
    local macpid=$!

    local t tt rc=0
    for t in "${TARGETS_LINUX[@]}"; do
        tt=$SECONDS
        if (cd "$work/src" && musl_build "$t") >"$work/$t.log" 2>&1; then
            mkdir -p "$work/bin/$t"
            cp "$CARGO_TARGET_DIR/$t/release/cpatop" "$work/bin/$t/cpatop"
            echo "  built $t ($(fmt_secs $((SECONDS - tt))))"
        else
            echo "  FAILED $t (log: $work/$t.log)"
            tail -n 20 "$work/$t.log" >&2
            rc=1
        fi
    done
    tt=$SECONDS
    if wait "$macpid"; then
        echo "  built ${TARGETS_MAC[*]} on $CPATOP_MAC_HOST (waited $(fmt_secs $((SECONDS - tt))) after Linux)"
    else
        echo "  FAILED macOS builds on $CPATOP_MAC_HOST (log: $work/mac.log)"
        tail -n 20 "$work/mac.log" >&2
        rc=1
    fi
    cp "$work"/*.log "$dist/" 2>/dev/null
    for f in "$dist"/*.log; do mv "$f" "$dist/build-$(basename "$f")"; done
    [ "$rc" = 0 ] || die "build failed; nothing published"

    # Package: keep asset names and layout in sync with install.sh.
    local name
    for t in "${TARGETS_LINUX[@]}" "${TARGETS_MAC[@]}"; do
        name="cpatop-$t"
        [ -x "$work/bin/$t/cpatop" ] || die "missing binary for $t"
        mkdir -p "$work/pkg/$name"
        cp "$work/bin/$t/cpatop" "$work/pkg/$name/cpatop"
        chmod 755 "$work/pkg/$name/cpatop"
        for f in "${PAYLOAD[@]}"; do cp "$work/src/$f" "$work/pkg/$name/"; done
        tar -C "$work/pkg" --owner=0 --group=0 --numeric-owner -czf "$dist/$name.tar.gz" "$name"
    done
    # install.sh looks up "<hash>  cpatop-<target>.tar.gz" lines here.
    (cd "$dist" && sha256sum -- *.tar.gz >SHA256SUMS)
    printf '%s\n' "$notes" >"$dist/RELEASE_NOTES.md"

    echo "release: packaged in $(fmt_secs $((SECONDS - t0))) -> $dist"
    (cd "$dist" && ls -l -- *.tar.gz SHA256SUMS)

    if [ "$dry" = 1 ]; then
        echo "release: dry run; not tagging or publishing $tag"
        return 0
    fi

    git tag -a "$tag" -m "cpatop $tag" "$sha" || die "git tag failed"
    git push "$remote" "refs/tags/$tag" || die "pushing tag $tag failed (delete it locally with: git tag -d $tag)"
    local pre=()
    [[ "$version" == *-* ]] && pre=(--prerelease)
    gh release create "$tag" --verify-tag --title "cpatop $tag" --notes-file "$dist/RELEASE_NOTES.md" \
        "${pre[@]}" "$dist"/*.tar.gz "$dist/SHA256SUMS" ||
        die "gh release create failed; the tag $tag is pushed — retry with: gh release create $tag --verify-tag --notes-file dist/RELEASE_NOTES.md dist/*.tar.gz dist/SHA256SUMS"
    echo "release: published $tag"
}

cleanup() {
    [ -n "${work:-}" ] && rm -rf "$work"
    return 0
}

# changelog_section <X.Y.Z|Unreleased>: body of "## [X.Y.Z]..." in CHANGELOG.md.
changelog_section() {
    awk -v v="$1" '
        /^## \[/ { if (on) exit; on = index($0, "## [" v "]") == 1; next }
        /^\[[^]]+\]: / { if (on) exit }
        on { print }
    ' CHANGELOG.md | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
}

# mac_build <sha>: build both macOS targets and the universal binary on the
# Mac, then copy the three binaries into $work/bin/<target>/cpatop.
mac_build() {
    local sha=$1 rdir
    rdir=$(mac_ship "$sha" "rel$$") || { echo "could not copy $sha to $CPATOP_MAC_HOST"; return 1; }
    # shellcheck disable=SC2016 # runs on the Mac
    mac_run "$rdir" '
        sw_vers -productVersion; cargo --version
        rustup target add aarch64-apple-darwin x86_64-apple-darwin
        for t in aarch64-apple-darwin x86_64-apple-darwin; do
            cargo build --release --locked --target "$t"
            mkdir -p "out/$t"
            cp "$CARGO_TARGET_DIR/$t/release/cpatop" "out/$t/cpatop"
        done
        mkdir -p out/universal-apple-darwin
        lipo -create -output out/universal-apple-darwin/cpatop \
            out/aarch64-apple-darwin/cpatop out/x86_64-apple-darwin/cpatop
        lipo -info out/universal-apple-darwin/cpatop
        for t in aarch64-apple-darwin x86_64-apple-darwin; do
            printf "%s: macOS >= " "$t"
            vtool -show-build "out/$t/cpatop" 2>/dev/null | awk "\$1 == \"minos\" || \$1 == \"version\" { print \$2; exit }"
        done' &&
        mac_ssh "COPYFILE_DISABLE=1 tar --no-mac-metadata --no-xattrs -C \"\$HOME/$rdir/out\" -cf - ." |
        tar -x -C "$work/bin"
    local rc=$?
    mac_cleanup "$rdir"
    return "$rc"
}

main "$@"
