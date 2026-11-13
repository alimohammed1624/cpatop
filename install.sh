#!/bin/sh
# cpatop installer.
#
#   curl -fsSL https://raw.githubusercontent.com/alimohammed1624/cpatop/main/install.sh | sh
#
# Downloads the prebuilt cpatop binary for this machine from the latest
# GitHub release, verifies it against the release's SHA256SUMS and installs
# it into ~/.local/bin (no sudo).
#
# Options (environment variables):
#   CPATOP_INSTALL_DIR  install directory (default: $HOME/.local/bin)
#   CPATOP_VERSION      release tag to install, e.g. v0.2.0 (default: latest)
#
# Asset names (cpatop-<target>.tar.gz containing cpatop-<target>/cpatop):
# keep asset names in sync with .github/workflows/release.yml.
# Releases also carry cpatop-universal-apple-darwin.tar.gz (one macOS binary
# for both architectures); this script deliberately installs the smaller
# architecture-specific archive instead. The README documents the universal
# one for manual download.
#
# POSIX sh only. Everything runs from main(), called on the last line, so a
# truncated download never executes a partial script.

set -eu

REPO="alimohammed1624/cpatop"
CARGO_HINT="cargo install --git https://github.com/$REPO --locked"
_cpatop_tmp=""

say() {
    printf '%s\n' "$*"
}

warn() {
    printf 'cpatop-install: warning: %s\n' "$*" >&2
}

err() {
    printf 'cpatop-install: error: %s\n' "$*" >&2
    exit 1
}

cleanup() {
    if [ -n "$_cpatop_tmp" ] && [ -d "$_cpatop_tmp" ]; then
        rm -rf "$_cpatop_tmp"
    fi
}

need() {
    command -v "$1" >/dev/null 2>&1 || err "'$1' is required but was not found on PATH"
}

# Prints the Rust target triple for this machine, or exits with an error.
detect_target() {
    _os=$(uname -s 2>/dev/null || echo unknown)
    _arch=$(uname -m 2>/dev/null || echo unknown)

    case "$_os" in
        Linux)
            case "$_arch" in
                x86_64 | amd64) echo "x86_64-unknown-linux-musl"; return ;;
                aarch64 | arm64) echo "aarch64-unknown-linux-musl"; return ;;
            esac
            ;;
        Darwin)
            # A shell running under Rosetta reports x86_64 on Apple silicon;
            # prefer the native arm64 build there.
            if [ "$_arch" = "x86_64" ] &&
                [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
                _arch=arm64
            fi
            case "$_arch" in
                x86_64 | amd64) echo "x86_64-apple-darwin"; return ;;
                aarch64 | arm64) echo "aarch64-apple-darwin"; return ;;
            esac
            ;;
    esac

    err "no prebuilt cpatop binary for $_os/$_arch.
  Build it from source instead (needs a Rust toolchain):
    $CARGO_HINT"
}

# download <url> <output-file>
download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$2" "$1" || err "download failed: $1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1" || err "download failed: $1"
    else
        err "need curl or wget to download cpatop"
    fi
}

# sha256 <file>: prints the hex digest.
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        err "need sha256sum or shasum to verify the download"
    fi
}

main() {
    need uname
    need tar
    need mktemp
    need awk
    if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
        err "need curl or wget to download cpatop"
    fi
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        err "need sha256sum or shasum to verify the download"
    fi

    target=$(detect_target)
    asset="cpatop-$target.tar.gz"

    version="${CPATOP_VERSION:-}"
    case "$version" in
        "" | latest) version="" ;;
        v*) ;;
        *) version="v$version" ;;
    esac

    if [ -n "${CPATOP_DOWNLOAD_BASE:-}" ]; then
        base="${CPATOP_DOWNLOAD_BASE%/}"
    elif [ -n "$version" ]; then
        base="https://github.com/$REPO/releases/download/$version"
    else
        base="https://github.com/$REPO/releases/latest/download"
    fi

    if [ -n "${CPATOP_INSTALL_DIR:-}" ]; then
        dir="$CPATOP_INSTALL_DIR"
    elif [ -n "${HOME:-}" ]; then
        dir="$HOME/.local/bin"
    else
        err "HOME is not set; set CPATOP_INSTALL_DIR to choose where to install"
    fi

    _cpatop_tmp=$(mktemp -d 2>/dev/null || mktemp -d -t cpatop-install) ||
        err "could not create a temporary directory"
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    say "Installing cpatop ${version:-(latest)} for $target"

    download "$base/$asset" "$_cpatop_tmp/$asset"
    download "$base/SHA256SUMS" "$_cpatop_tmp/SHA256SUMS"

    expected=$(awk -v n="$asset" '$2 == n || $2 == "*" n { print $1; exit }' "$_cpatop_tmp/SHA256SUMS")
    [ -n "$expected" ] || err "$asset is not listed in SHA256SUMS; refusing to install"
    actual=$(sha256 "$_cpatop_tmp/$asset")
    if [ "$expected" != "$actual" ]; then
        err "checksum mismatch for $asset; refusing to install.
  expected: $expected
  got:      $actual"
    fi
    say "Checksum verified."

    tar -xzf "$_cpatop_tmp/$asset" -C "$_cpatop_tmp" || err "could not extract $asset"
    bin="$_cpatop_tmp/cpatop-$target/cpatop"
    [ -f "$bin" ] || err "$asset does not contain cpatop-$target/cpatop"

    case "$dir" in
        /*) ;;
        *) dir="$(pwd)/$dir" ;;
    esac
    mkdir -p "$dir" || err "could not create $dir"
    staged="$dir/.cpatop.install.$$"
    if ! cp "$bin" "$staged" || ! chmod 755 "$staged" || ! mv -f "$staged" "$dir/cpatop"; then
        rm -f "$staged"
        err "could not install cpatop into $dir (is it writable?)"
    fi

    installed=$("$dir/cpatop" --version 2>/dev/null) || installed=""
    if [ -n "$installed" ]; then
        say "Installed $installed to $dir/cpatop"
    else
        warn "installed $dir/cpatop, but '$dir/cpatop --version' failed"
    fi

    case ":${PATH:-}:" in
        *":$dir:"*) ;;
        *)
            path_dir="$dir"
            if [ -n "${HOME:-}" ]; then
                case "$dir" in
                    "$HOME"/*) path_dir="\$HOME${dir#"$HOME"}" ;;
                esac
            fi
            say ""
            say "$dir is not on your PATH. Add this line to your shell profile"
            say "(e.g. ~/.profile, ~/.bashrc or ~/.zshrc), then open a new terminal:"
            say ""
            say "    export PATH=\"$path_dir:\$PATH\""
            ;;
    esac

    say ""
    say "Next, point it at your CPA Usage Keeper:"
    say "    cpatop --base-url http://localhost:8318 --password-file ~/.config/cpatop/admin.pw"
    say "Run 'cpatop --help' for all options."
}

main "$@"
