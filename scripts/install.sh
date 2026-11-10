#!/bin/sh
# Installs tos from a GitHub release, without Rust and without sudo:
#
#   curl -fsSL https://github.com/hugegene/termos/releases/latest/download/install.sh | sh
#
# Settings, all optional:
#   TOS_VERSION       a version such as 0.2.0 (default: the latest release)
#   TOS_INSTALL_DIR   where tos goes (default: ~/.local/bin)
#   TOS_DOWNLOAD_URL  the folder holding the release files (default: GitHub; used for testing)
#
# Everything is in functions and `main` runs on the last line, so a download cut short runs nothing.
set -eu

REPO=hugegene/termos

say() { printf 'tos install: %s\n' "$*"; }
die() {
    printf 'tos install: error: %s\n' "$*" >&2
    exit 1
}

# Prints the release target for this machine, or stops with the reason it isn't supported.
detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
    Linux)
        case "$arch" in
        x86_64 | amd64) echo x86_64-unknown-linux-gnu ;;
        aarch64 | arm64) echo aarch64-unknown-linux-gnu ;;
        *) die "Linux on $arch isn't supported. There are builds for x86_64 and arm64 (aarch64)." ;;
        esac
        ;;
    Darwin)
        # A shell running under Rosetta says x86_64 on an Apple Silicon Mac.
        if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
            arch=arm64
        fi
        case "$arch" in
        arm64) echo aarch64-apple-darwin ;;
        *) die "Intel Macs aren't supported. tos needs a Mac with Apple Silicon (M1 or later)." ;;
        esac
        ;;
    *) die "$os isn't supported. tos runs on Linux and macOS." ;;
    esac
}

download() { # url file
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$2" "$1" || die "couldn't download $1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1" || die "couldn't download $1"
    else
        die "needs curl or wget to download tos"
    fi
}

sha256_of() { # file
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

main() {
    target=$(detect_target)
    command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 ||
        die "needs sha256sum or shasum to check the download"
    command -v tar >/dev/null 2>&1 || die "needs tar to unpack the download"

    install_dir=${TOS_INSTALL_DIR:-$HOME/.local/bin}
    if [ -n "${TOS_DOWNLOAD_URL:-}" ]; then
        base=${TOS_DOWNLOAD_URL%/}
    elif [ -n "${TOS_VERSION:-}" ]; then
        base=https://github.com/$REPO/releases/download/v${TOS_VERSION#v}
    else
        base=https://github.com/$REPO/releases/latest/download
    fi
    file=tos-$target.tar.gz

    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t tos)
    trap 'rm -rf "$tmp"' EXIT

    say "downloading $base/$file"
    download "$base/$file" "$tmp/$file"
    download "$base/$file.sha256" "$tmp/$file.sha256"
    want=$(cut -d ' ' -f 1 <"$tmp/$file.sha256")
    got=$(sha256_of "$tmp/$file")
    if [ -z "$want" ] || [ "$want" != "$got" ]; then
        die "the download is damaged: $file has sha256 $got, but the release says ${want:-nothing}"
    fi

    tar -xzf "$tmp/$file" -C "$tmp"
    set -- "$tmp"/tos-*/tos
    [ -f "$1" ] || die "$file has no tos in it"
    new=$1
    # Run it before installing, so a binary this system can't run never replaces a working one.
    version=$("$new" --version 2>/dev/null) || die "the downloaded tos doesn't run on this system.
On Linux it needs glibc 2.34 or newer: Ubuntu 22.04+, Debian 12+, RHEL 9+, Fedora 35+."

    mkdir -p "$install_dir"
    upgrade=no
    [ -e "$install_dir/tos" ] && upgrade=yes
    # Copy next to the old one, then rename over it: a tos that's running keeps its old file.
    cp "$new" "$install_dir/.tos.new.$$"
    chmod 755 "$install_dir/.tos.new.$$"
    mv -f "$install_dir/.tos.new.$$" "$install_dir/tos"
    say "installed $version as $install_dir/tos"

    case ":$PATH:" in
    *":$install_dir:"*) ;;
    *) say "$install_dir isn't on your PATH. Add this line to your shell's startup file (~/.bashrc, ~/.zshrc):
    export PATH=\"$install_dir:\$PATH\"" ;;
    esac
    # Up to 0.1.1 the command was called tb.
    if [ -e "$install_dir/tb" ]; then
        say "$install_dir/tb is this program from before it was renamed tos. Sessions started by tb keep
    running under it: end them with \`tb kill-all-sessions -y\` and \`tos\` restores them. Then: rm $install_dir/tb"
    fi
    if [ "$upgrade" = yes ] && command -v pgrep >/dev/null 2>&1 && pgrep -x tos >/dev/null 2>&1; then
        say "a tos server from before is still running the old version. If tos refuses to attach,
    run: tos kill   (attaching again restores the session)"
    fi
}

main "$@"
