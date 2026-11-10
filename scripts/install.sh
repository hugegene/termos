#!/bin/sh
# Installs tos from a GitHub release, without Rust and without sudo. Each version goes in
# ~/.local/share/termos/versions/<version>/, and $TOS_INSTALL_DIR/tos (~/.local/bin/tos) links to it.
# `tos update` runs this script again:
#
#   curl -fsSL https://github.com/hugegene/termos/releases/latest/download/install.sh | sh
#
# Settings, all optional:
#   TOS_VERSION       a version such as 0.2.0 (default: the latest release)
#   TOS_INSTALL_DIR   where tos goes (default: ~/.local/bin)
#   TOS_DOWNLOAD_URL  the folder holding the release files (default: GitHub; used for testing)
#   TOS_NO_MODIFY_PATH=1  don't add the folder to your shell's startup file; only say what to add
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

# The startup file of the user's login shell, where the folder is added to PATH.
startup_file() {
    case "${SHELL:-}" in
    */zsh) echo "${ZDOTDIR:-$HOME}/.zshrc" ;;
    */bash)
        # Terminal on macOS opens login shells, and bash's login shells don't read ~/.bashrc.
        if [ "$(uname -s)" = Darwin ]; then echo "$HOME/.bash_profile"; else echo "$HOME/.bashrc"; fi
        ;;
    */fish) echo "${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/termos.fish" ;;
    *) echo "$HOME/.profile" ;;
    esac
}

MARK="# Added by the termos installer (tos). TOS_NO_MODIFY_PATH=1 skips this."

# Makes new shells find tos: adds the folder to the startup file, unless it's already on PATH or
# TOS_NO_MODIFY_PATH=1. The line only adds the folder when it's missing, so on Ubuntu, whose ~/.profile adds
# ~/.local/bin at login, it doesn't appear twice.
add_to_path() { # folder
    dir=$1
    case ":$PATH:" in *":$dir:"*) return 0 ;; esac
    case "$dir" in
    "$HOME"/*) shown="\$HOME/${dir#"$HOME"/}" ;;
    *) shown=$dir ;;
    esac
    rc=$(startup_file)
    case "$rc" in
    *.fish)
        line="fish_add_path \"$shown\""
        now="fish_add_path \"$dir\""
        ;;
    *)
        line="case \":\$PATH:\" in *\":$shown:\"*) ;; *) export PATH=\"$shown:\$PATH\" ;; esac"
        now="export PATH=\"$dir:\$PATH\""
        ;;
    esac
    if [ "${TOS_NO_MODIFY_PATH:-0}" != 1 ]; then
        if [ -f "$rc" ] && grep -qF "$MARK" "$rc"; then
            say "$rc already adds $dir to PATH. Open a new terminal, or run this in this one:
    $now"
            return 0
        fi
        if { mkdir -p "$(dirname "$rc")" && printf '\n%s\n%s\n' "$MARK" "$line" >>"$rc"; } 2>/dev/null; then
            say "added $dir to PATH in $rc. Open a new terminal, or run this in this one:
    $now"
            return 0
        fi
        say "couldn't write to $rc."
    fi
    say "$dir isn't on your PATH. Add this to $rc:
    $now"
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

    # Each version gets its own folder, like Claude Code's, and $install_dir/tos is a symlink to the current
    # one. `tos update` runs this script again.
    number=$(printf '%s\n' "$version" | cut -d ' ' -f 2)
    case "$number" in
    "" | */* | .*) die "the downloaded tos gave an unexpected version: $version" ;;
    esac
    data=${XDG_DATA_HOME:-$HOME/.local/share}/termos
    dest=$data/versions/$number
    mkdir -p "$dest" "$install_dir"
    upgrade=no
    previous=
    if [ -L "$install_dir/tos" ]; then
        upgrade=yes
        previous=$(readlink "$install_dir/tos")
    elif [ -e "$install_dir/tos" ]; then
        upgrade=yes # a plain file, from 0.2.x
    fi
    # Copy, then rename into place, so a tos that's running keeps its file.
    cp "$new" "$dest/.tos.new.$$"
    chmod 755 "$dest/.tos.new.$$"
    mv -f "$dest/.tos.new.$$" "$dest/tos"
    # Switch the symlink in one step: make a new one, then rename it over the old.
    ln -s "$dest/tos" "$install_dir/.tos.link.$$"
    mv -f "$install_dir/.tos.link.$$" "$install_dir/tos"
    printf '%s\n' "$install_dir" >"$data/install-dir"
    # Keep this version and the one before it: a session may still be running that one.
    for old in "$data/versions"/*; do
        [ -d "$old" ] || continue
        [ "$old" = "$dest" ] && continue
        [ -n "$previous" ] && [ "$old" = "${previous%/tos}" ] && continue
        rm -rf "$old"
    done
    say "installed $version as $install_dir/tos (in $dest)"

    add_to_path "$install_dir"
    # Up to 0.1.1 the command was called tb.
    if [ -e "$install_dir/tb" ]; then
        say "$install_dir/tb is this program from before it was renamed tos. Sessions started by tb keep
    running under it: end them with \`tb kill-all-sessions -y\` and \`tos\` restores them. Then: rm $install_dir/tb"
    fi
    # Only when the version changed: reinstalling the same one leaves running servers as current as before.
    if [ "$upgrade" = yes ] && [ "${previous%/tos}" != "$dest" ] &&
        command -v pgrep >/dev/null 2>&1 && pgrep -x tos >/dev/null 2>&1; then
        say "a tos server from before is still running the old version. If tos refuses to attach,
    run: tos kill   (attaching again restores the session)"
    fi
}

main "$@"
