#!/bin/sh
# Install a verified release. Keep execution inside main so a truncated download
# cannot run a partially received script when this file is piped into sh.
set -eu

fail() {
    printf 'mdterm: %s\n' "$*" >&2
    exit 1
}

cleanup() {
    if [ -n "$mdterm_staged" ]; then rm -f "$mdterm_staged"; fi
    if [ -n "$mdterm_tmp" ]; then rm -rf "$mdterm_tmp"; fi
}

download() {
    curl --fail --silent --show-error --location \
        --proto '=https' --proto-redir '=https' \
        --connect-timeout 15 --max-time 120 --retry 2 \
        --output "$2" "$1"
}

main() {
    if [ "$#" -ne 0 ]; then
        if [ "$#" -eq 1 ] && [ "$1" = '--help' ]; then
            printf '%s\n' 'Install mdterm from GitHub Releases.' \
                'MDTERM_VERSION=v0.1.0 selects a version (default: latest stable).' \
                'MDTERM_INSTALL_DIR selects a directory (default: $HOME/.local/bin).'
            return
        fi
        fail 'Unknown argument. Use --help for installation options.'
    fi

    mdterm_tmp=''
    mdterm_staged=''
    trap cleanup 0
    trap 'exit 1' HUP INT TERM

    for mdterm_tool in curl tar mktemp awk grep chmod mkdir mv cat uname; do
        command -v "$mdterm_tool" >/dev/null 2>&1 || fail "Required tool is missing: $mdterm_tool"
    done
    if command -v sha256sum >/dev/null 2>&1; then
        mdterm_hash_tool=sha256sum
    elif command -v shasum >/dev/null 2>&1; then
        mdterm_hash_tool=shasum
    else
        fail 'Install sha256sum or shasum to verify the download.'
    fi

    mdterm_os=$(uname -s)
    mdterm_arch=$(uname -m)
    case "$mdterm_arch" in
        x86_64|amd64) mdterm_arch=x86_64 ;;
        aarch64|arm64) mdterm_arch=aarch64 ;;
        *) fail "Unsupported architecture: $mdterm_arch (supported: x86-64 and ARM64)" ;;
    esac
    case "$mdterm_os" in
        Linux) mdterm_target="$mdterm_arch-unknown-linux-musl" ;;
        Darwin) mdterm_target="$mdterm_arch-apple-darwin" ;;
        *) fail "Unsupported operating system: $mdterm_os (supported: Linux and macOS)" ;;
    esac

    mdterm_repo='https://github.com/mfontcada/mdterm'
    mdterm_tag=${MDTERM_VERSION:-}
    if [ -z "$mdterm_tag" ]; then
        mdterm_latest=$(curl --fail --silent --show-error --location \
            --proto '=https' --proto-redir '=https' \
            --connect-timeout 15 --max-time 120 --retry 2 \
            --output /dev/null --write-out '%{url_effective}' \
            "$mdterm_repo/releases/latest") || fail 'Could not find the latest release. Check your connection and the releases page.'
        case "$mdterm_latest" in
            "$mdterm_repo/releases/tag/"*) mdterm_tag=${mdterm_latest##*/} ;;
            *) fail 'GitHub did not return a release version.' ;;
        esac
    fi
    case "$mdterm_tag" in v*) ;; *) mdterm_tag="v$mdterm_tag" ;; esac
    printf '%s\n' "$mdterm_tag" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' \
        || fail 'MDTERM_VERSION must be a stable version such as v0.1.0.'

    mdterm_asset="mdterm-$mdterm_tag-$mdterm_target.tar.gz"
    mdterm_base="$mdterm_repo/releases/download/$mdterm_tag"
    mdterm_tmp=$(mktemp -d "${TMPDIR:-/tmp}/mdterm.XXXXXXXX") || fail 'Could not create a temporary directory.'
    download "$mdterm_base/$mdterm_asset" "$mdterm_tmp/archive.tar.gz" \
        || fail "Could not download $mdterm_asset. Check the version and your connection."
    download "$mdterm_base/SHA256SUMS" "$mdterm_tmp/SHA256SUMS" \
        || fail 'Could not download release checksums.'
    mdterm_expected=$(awk -v name="$mdterm_asset" \
        '$2 == name { count++; digest = $1 } END { if (count != 1) exit 1; print digest }' \
        "$mdterm_tmp/SHA256SUMS") || fail 'The archive has no unique checksum in this release.'
    printf '%s\n' "$mdterm_expected" | grep -Eq '^[0-9a-f]{64}$' || fail 'Invalid release checksum.'
    if [ "$mdterm_hash_tool" = sha256sum ]; then
        mdterm_actual=$(sha256sum < "$mdterm_tmp/archive.tar.gz")
    else
        mdterm_actual=$(shasum -a 256 < "$mdterm_tmp/archive.tar.gz")
    fi
    mdterm_actual=${mdterm_actual%% *}
    [ "$mdterm_actual" = "$mdterm_expected" ] || fail 'Checksum mismatch. The existing installation was not changed.'

    tar -xzf "$mdterm_tmp/archive.tar.gz" -C "$mdterm_tmp" mdterm || fail 'Could not extract the executable.'
    [ -f "$mdterm_tmp/mdterm" ] && [ ! -L "$mdterm_tmp/mdterm" ] || fail 'The archive does not contain a regular executable.'
    chmod 755 "$mdterm_tmp/mdterm"
    mdterm_reported=$("$mdterm_tmp/mdterm" --version) || fail 'The downloaded executable cannot run on this system.'
    [ "$mdterm_reported" = "mdterm ${mdterm_tag#v}" ] || fail 'The executable version does not match the release.'

    if [ -n "${MDTERM_INSTALL_DIR:-}" ]; then
        mdterm_destination=$MDTERM_INSTALL_DIR
    else
        [ -n "${HOME:-}" ] || fail 'HOME is unset; set MDTERM_INSTALL_DIR explicitly.'
        mdterm_destination="$HOME/.local/bin"
    fi
    mkdir -p "$mdterm_destination" || fail "Cannot create $mdterm_destination"
    mdterm_destination=$(cd "$mdterm_destination" && pwd -P)
    [ ! -d "$mdterm_destination/mdterm" ] || fail 'The installation path is a directory.'
    mdterm_staged=$(mktemp "$mdterm_destination/.mdterm.XXXXXXXX") || fail 'The installation directory is not writable.'
    cat "$mdterm_tmp/mdterm" > "$mdterm_staged"
    chmod 755 "$mdterm_staged"
    mv -f "$mdterm_staged" "$mdterm_destination/mdterm" || fail 'Could not replace the executable.'
    mdterm_staged=''
    printf 'Installed %s to %s/mdterm\n' "$mdterm_reported" "$mdterm_destination"

    mdterm_found=$(command -v mdterm 2>/dev/null || true)
    if [ "$mdterm_found" != "$mdterm_destination/mdterm" ]; then
        printf 'Add %s at the beginning of your PATH to use this installation.\n' "$mdterm_destination"
        if [ -z "${MDTERM_INSTALL_DIR:-}" ]; then
            printf '%s\n' 'For this shell: export PATH="$HOME/.local/bin:$PATH"'
        fi
        if [ -n "$mdterm_found" ]; then
            printf 'Your shell currently finds: %s\n' "$mdterm_found"
        fi
    fi
}

main "$@"
