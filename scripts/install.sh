#!/bin/sh
# Zenith CLI installer
#
# Install latest stable:
#   curl -fsSL https://raw.githubusercontent.com/farhan-syah/zenith/main/scripts/install.sh | sh
#
# Install latest prerelease:
#   curl -fsSL https://raw.githubusercontent.com/farhan-syah/zenith/main/scripts/install.sh | sh -s -- --pre
#
# Install/switch to a specific version:
#   curl -fsSL https://raw.githubusercontent.com/farhan-syah/zenith/main/scripts/install.sh | sh -s -- --version v0.1.0
#   curl -fsSL https://raw.githubusercontent.com/farhan-syah/zenith/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
#
# Build and install from a local source checkout:
#   ./scripts/install.sh --local
#
# Uninstall:
#   curl -fsSL https://raw.githubusercontent.com/farhan-syah/zenith/main/scripts/install.sh | sh -s -- --uninstall
#
# Environment variables:
#   ZENITH_INSTALL_DIR  Install directory (default: ~/.local/bin)

set -eu

REPO="farhan-syah/zenith"
BINARY="zenith"
INSTALL_DIR="${ZENITH_INSTALL_DIR:-$HOME/.local/bin}"

main() {
    action="install"
    version="latest"
    channel="stable"
    local_build="false"
    modify_path="true"

    while [ $# -gt 0 ]; do
        case "$1" in
            --help|-h)        usage; exit 0 ;;
            --uninstall)      action="uninstall"; shift ;;
            --pre)            channel="pre"; shift ;;
            --version)        version="$2"; channel="exact"; shift 2 ;;
            --local)          local_build="true"; shift ;;
            --no-modify-path) modify_path="false"; shift ;;
            *)                echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
        esac
    done

    if [ "$local_build" = "true" ] && [ "$channel" != "stable" ]; then
        echo "Error: --local cannot be combined with --version or --pre." >&2
        exit 1
    fi

    case "$action" in
        install)
            if [ "$local_build" = "true" ]; then
                do_install_local
            else
                do_install "$version" "$channel"
            fi
            ;;
        uninstall) do_uninstall ;;
    esac
}

usage() {
    cat <<EOF
Zenith CLI installer

Usage:
  install.sh [OPTIONS]

Options:
  --pre              Install the latest prerelease version
  --version VERSION  Install a specific version (e.g., v0.1.0, v0.1.0-beta.1)
  --local            Build from the current source checkout and install that
                     binary (requires cargo; mutually exclusive with --version
                     and --pre)
  --no-modify-path   Install the binary but do not edit your shell profile to
                     add the install directory to PATH
  --uninstall        Remove zenith from the install directory
  --help, -h         Show this help message

Examples:
  install.sh                        Install latest stable release
  install.sh --pre                  Install latest prerelease
  install.sh --version v0.1.0       Switch to a specific stable version
  install.sh --version v0.2.0-rc.1  Switch to a specific prerelease
  install.sh --local                Build from this checkout and install it
  install.sh --local --no-modify-path   Build + install, leave PATH untouched
  install.sh --uninstall            Remove zenith

Environment:
  ZENITH_INSTALL_DIR  Override install directory (default: ~/.local/bin)
EOF
}

do_install() {
    version="$1"
    channel="$2"

    check_prereqs

    os="$(detect_os)"
    arch="$(detect_arch)"
    target="$(detect_target "$os" "$arch")"

    if [ "$version" = "latest" ]; then
        case "$channel" in
            stable) version="$(fetch_latest_stable)" ;;
            pre)    version="$(fetch_latest_pre)" ;;
        esac
        if [ -z "$version" ]; then
            echo "Error: no ${channel} release found." >&2
            echo "Check https://github.com/${REPO}/releases" >&2
            exit 1
        fi
    fi

    current=""
    if command -v "$BINARY" > /dev/null 2>&1; then
        current="$("$BINARY" --version 2>/dev/null | awk '{print $2}' || echo "")"
    fi

    requested="${version#v}"
    if [ "$current" = "$requested" ]; then
        echo "zenith ${requested} is already installed."
        exit 0
    fi

    if [ -n "$current" ]; then
        echo "Switching zenith ${current} -> ${requested} (${target})..."
    else
        echo "Installing zenith ${requested} (${target})..."
    fi

    ext="tar.gz"
    bin_name="$BINARY"
    if [ "$os" = "windows" ]; then
        ext="zip"
        bin_name="${BINARY}.exe"
    fi

    url="https://github.com/${REPO}/releases/download/${version}/zenith-${requested}-${target}.${ext}"

    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    echo "Downloading from GitHub Releases..."
    download "$url" "$tmpdir/archive.${ext}"

    if [ "$os" = "windows" ]; then
        unzip -oq "$tmpdir/archive.zip" -d "$tmpdir"
    else
        tar xzf "$tmpdir/archive.tar.gz" -C "$tmpdir"
    fi

    if [ ! -f "$tmpdir/$bin_name" ]; then
        echo "Error: binary not found in archive" >&2
        exit 1
    fi

    place_binary "$tmpdir/$bin_name" "$bin_name"

    echo ""
    echo "Installed zenith to $(display_path "$INSTALL_DIR/$bin_name")"
    "${INSTALL_DIR}/${bin_name}" --version 2>/dev/null || true

    setup_path

    print_get_started
}

# Install from a local source checkout: build with cargo, then place the binary.
do_install_local() {
    root="$(repo_root)"
    if [ -z "$root" ]; then
        echo "Error: --local must be run from a Zenith source checkout (e.g. ./scripts/install.sh --local)" >&2
        exit 1
    fi

    if ! command -v cargo > /dev/null 2>&1; then
        echo "Error: cargo not found. Install Rust from https://rustup.rs and try again." >&2
        exit 1
    fi

    bin_name="$BINARY"
    if [ "$(detect_os)" = "windows" ]; then
        bin_name="${BINARY}.exe"
    fi

    target_dir="${CARGO_TARGET_DIR:-$root/target}"
    built_bin="$target_dir/release/$bin_name"

    echo "Building zenith from $(display_path "$root") (cargo build --release)..."
    ( cd "$root" && cargo build --release --bin "$BINARY" )

    if [ ! -f "$built_bin" ]; then
        echo "Error: built binary not found at $(display_path "$built_bin")" >&2
        exit 1
    fi

    place_binary "$built_bin" "$bin_name"

    echo ""
    echo "Installed zenith to $(display_path "$INSTALL_DIR/$bin_name")"
    "${INSTALL_DIR}/${bin_name}" --version 2>/dev/null || true

    setup_path

    print_get_started
}

# Resolve the repo root from this script's location (<root>/scripts/install.sh).
# Prints nothing if the script path can't be resolved (e.g. piped via curl | sh).
repo_root() {
    case "$0" in
        */*) script_dir="$(cd "$(dirname "$0")" 2>/dev/null && pwd)" ;;
        *)   script_dir="" ;;
    esac
    if [ -z "$script_dir" ] || [ ! -f "$script_dir/install.sh" ]; then
        return
    fi
    ( cd "$script_dir/.." 2>/dev/null && pwd )
}

# Place a binary at INSTALL_DIR with execute permission, using sudo if needed.
place_binary() {
    src="$1"
    bin_name="$2"
    mkdir -p "$INSTALL_DIR" 2>/dev/null || true
    if [ -w "$INSTALL_DIR" ]; then
        cp "$src" "$INSTALL_DIR/$bin_name"
        chmod +x "$INSTALL_DIR/$bin_name" 2>/dev/null || true
    else
        sudo mkdir -p "$INSTALL_DIR"
        sudo cp "$src" "$INSTALL_DIR/$bin_name"
        sudo chmod +x "$INSTALL_DIR/$bin_name" 2>/dev/null || true
    fi
}

print_get_started() {
    echo ""
    echo "Get started:"
    echo "  zenith --help"
    echo "  zenith validate document.zen"
    echo "  zenith render document.zen --png out.png"
}

do_uninstall() {
    os="$(detect_os)"
    bin_name="$BINARY"
    if [ "$os" = "windows" ]; then
        bin_name="${BINARY}.exe"
    fi

    target="${INSTALL_DIR}/${bin_name}"
    if [ ! -f "$target" ]; then
        echo "zenith is not installed at $(display_path "$target")"
        exit 0
    fi

    if [ -w "$target" ]; then
        rm "$target"
    else
        sudo rm "$target"
    fi

    echo "Uninstalled zenith from $(display_path "$target")"
}

check_prereqs() {
    missing=""
    command -v curl > /dev/null 2>&1 || command -v wget > /dev/null 2>&1 || missing="curl or wget"

    os="$(detect_os)"
    if [ "$os" = "windows" ]; then
        command -v unzip > /dev/null 2>&1 || missing="${missing:+$missing, }unzip"
    else
        command -v tar > /dev/null 2>&1 || missing="${missing:+$missing, }tar"
    fi

    if [ -n "$missing" ]; then
        echo "Error: missing required tools: ${missing}" >&2
        exit 1
    fi
}

setup_path() {
    if [ "$modify_path" = "false" ]; then
        case ":${PATH}:" in
            *":${INSTALL_DIR}:"*) ;;
            *) echo ""
               echo "Note: --no-modify-path set; ${INSTALL_DIR} was not added to PATH."
               echo "Add it yourself, or run: export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
        esac
        return
    fi

    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) return ;;
    esac

    shell_name="$(basename "${SHELL:-/bin/sh}")"
    line="export PATH=\"${INSTALL_DIR}:\$PATH\""

    case "$shell_name" in
        zsh)  rc="$HOME/.zshrc" ;;
        bash)
            if [ -f "$HOME/.bashrc" ]; then
                rc="$HOME/.bashrc"
            else
                rc="$HOME/.bash_profile"
            fi
            ;;
        fish)
            line="fish_add_path ${INSTALL_DIR}"
            rc="$HOME/.config/fish/config.fish"
            ;;
        *)    rc="" ;;
    esac

    if [ -n "$rc" ]; then
        if [ -f "$rc" ] && grep -qF "$INSTALL_DIR" "$rc" 2>/dev/null; then
            return
        fi
        echo "$line" >> "$rc"
        echo ""
        echo "Added $(display_path "$INSTALL_DIR") to PATH in $(display_path "$rc")"
        echo "Restart your shell or run: $line"
    else
        echo ""
        echo "Add $(display_path "$INSTALL_DIR") to your PATH:"
        echo "  $line"
    fi
}

download() {
    url="$1"
    dest="$2"
    if command -v curl > /dev/null 2>&1; then
        if ! curl -fsSL "$url" -o "$dest"; then
            echo "Error: download failed. Check the version and try again." >&2
            echo "  ${url}" >&2
            exit 1
        fi
    elif command -v wget > /dev/null 2>&1; then
        if ! wget -qO "$dest" "$url"; then
            echo "Error: download failed. Check the version and try again." >&2
            echo "  ${url}" >&2
            exit 1
        fi
    fi
}

display_path() {
    echo "$1" | sed "s|^$HOME|~|"
}

detect_os() {
    case "$(uname -s)" in
        Linux*)               echo "linux" ;;
        Darwin*)              echo "macos" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *) echo "Error: unsupported OS: $(uname -s)" >&2; exit 1 ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) echo "Error: unsupported architecture: $(uname -m)" >&2; exit 1 ;;
    esac
}

detect_target() {
    os="$1"
    arch="$2"
    case "${os}-${arch}" in
        linux-x86_64)    echo "linux-x64" ;;
        linux-aarch64)   echo "linux-arm64" ;;
        macos-x86_64)    echo "macos-x64" ;;
        macos-aarch64)   echo "macos-arm64" ;;
        windows-x86_64)  echo "windows-x64" ;;
        *) echo "Error: unsupported platform: ${os}-${arch}" >&2; exit 1 ;;
    esac
}

# Fetch latest stable release (skips prereleases)
fetch_latest_stable() {
    _fetch_releases | _filter_stable | head -1
}

# Fetch latest prerelease
fetch_latest_pre() {
    _fetch_releases | _filter_pre | head -1
}

_fetch_releases() {
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL "https://api.github.com/repos/${REPO}/releases?per_page=20" | grep '"tag_name"' | cut -d'"' -f4
    elif command -v wget > /dev/null 2>&1; then
        wget -qO- "https://api.github.com/repos/${REPO}/releases?per_page=20" | grep '"tag_name"' | cut -d'"' -f4
    fi
}

# Tags without hyphen after version (v0.1.0, v1.0.0 — not v0.1.0-beta.1)
_filter_stable() {
    grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$'
}

# Tags with hyphen (v0.1.0-beta.1, v0.2.0-rc.1)
_filter_pre() {
    grep -E '^v[0-9]+\.[0-9]+\.[0-9]+-'
}

main "$@"
