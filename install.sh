#!/bin/sh
#
# Installs LiNa SM2 - Mod Launcher for the current user.
#
#   curl -fsSL https://raw.githubusercontent.com/Carlos17Kopra/linasm2-modloader/main/install.sh | sh
#
# It downloads the released binary for this machine, checks it against the
# SHA256SUMS of the same release, and puts it, a desktop entry and an icon
# under $HOME. No root, no package manager, nothing outside $HOME.
#
#   --uninstall   removes those three files again; settings, profiles and
#                 save backups are left alone
#   --force       installs the release even when the same or a newer
#                 version is already there
#   --version X   installs that version instead of the latest one
#
# POSIX sh on purpose: this is the first thing that runs on a machine
# nothing is known about, so it must not assume bash is installed, let
# alone that /bin/sh is bash.
#
# Every step that writes is placed so that an interruption leaves the
# previous installation intact: the download and the checksum come first,
# the binary is replaced by a rename (atomic, and safe while the old one
# is still running), and the desktop entry follows the binary it points to.

set -eu

REPO="${LINA_SM2_REPO:-Carlos17Kopra/linasm2-modloader}"
BIN_NAME="lina-sm2"
DISPLAY_NAME="LiNa SM2 - Mod Launcher"

# The two endpoints, separated out so the test harness in
# `packaging/test-install.sh` can point them at a directory of its own and
# run this script end to end without a network.
API_BASE="${LINA_SM2_API_BASE:-https://api.github.com}"
DOWNLOAD_BASE="${LINA_SM2_DOWNLOAD_BASE:-https://github.com/$REPO/releases/download}"

BIN_DIR="${LINA_SM2_BIN_DIR:-$HOME/.local/bin}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
DESKTOP_DIR="$DATA_HOME/applications"
ICON_DIR="$DATA_HOME/icons/hicolor/scalable/apps"

DESKTOP_FILE="$DESKTOP_DIR/$BIN_NAME.desktop"
ICON_FILE="$ICON_DIR/$BIN_NAME.svg"

say() { printf '%s\n' "$*"; }
step() { printf '  %s\n' "$*"; }

# Errors go to stderr so that `| sh` still shows them when stdout is being
# read by something else.
die() {
    printf '%s: %s\n' "$BIN_NAME" "$*" >&2
    exit 1
}

have() { command -v "$1" >/dev/null 2>&1; }

usage() {
    cat <<EOF
$DISPLAY_NAME — installer

Usage: install.sh [--uninstall] [--force] [--version VERSION]

  --uninstall      Remove binary, desktop entry and icon. Settings,
                   profiles and save backups are kept.
  --force          Install even if the same or a newer version is present.
  --version VER    Install this version (e.g. 0.2.0) instead of the latest.
  -h, --help       Show this text.

Installs to: $BIN_DIR
EOF
}

# ---------------------------------------------------------------- arguments

action="install"
force="no"
wanted_version=""

while [ $# -gt 0 ]; do
    case "$1" in
        --uninstall) action="uninstall" ;;
        --force) force="yes" ;;
        --version)
            [ $# -ge 2 ] || die "--version needs a version number"
            wanted_version="$2"
            shift
            ;;
        --version=*) wanted_version="${1#--version=}" ;;
        -h | --help)
            usage
            exit 0
            ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
    shift
done

# ------------------------------------------------------------------ helpers

download() {
    # $1 url, $2 destination. Both tools are told to fail loudly on an HTTP
    # error rather than writing the error page to the destination file —
    # without curl's -f, a 404 lands on disk as a perfectly valid tarball
    # name holding HTML.
    if have curl; then
        curl -fsSL "$1" -o "$2"
    elif have wget; then
        wget -q -O "$2" "$1"
    else
        die "neither curl nor wget found — one of them is needed to download"
    fi
}

# The version of an installation already present, or nothing.
installed_version() {
    [ -x "$BIN_DIR/$BIN_NAME" ] || return 0
    "$BIN_DIR/$BIN_NAME" --version 2>/dev/null | awk 'NR == 1 { print $NF }'
}

# Prints the higher of two version numbers. `sort -V` understands that 0.10
# comes after 0.9, which a plain string comparison does not.
higher_version() {
    printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1
}

refresh_desktop_caches() {
    # Both are optional: on a desktop without them the entry still works,
    # it may just take until the next login to appear in the menu. Neither
    # failure is worth aborting a finished installation for.
    if have update-desktop-database; then
        update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
    fi
    if have gtk-update-icon-cache; then
        gtk-update-icon-cache -q -t "$DATA_HOME/icons/hicolor" >/dev/null 2>&1 || true
    fi
}

# ---------------------------------------------------------------- uninstall

if [ "$action" = "uninstall" ]; then
    removed="no"
    for file in "$BIN_DIR/$BIN_NAME" "$DESKTOP_FILE" "$ICON_FILE"; do
        if [ -e "$file" ]; then
            rm -f "$file"
            step "removed $file"
            removed="yes"
        fi
    done

    if [ "$removed" = "no" ]; then
        say "$DISPLAY_NAME is not installed in $BIN_DIR — nothing to do."
        exit 0
    fi

    refresh_desktop_caches
    say ""
    say "$DISPLAY_NAME removed."
    # Named explicitly rather than deleted: these hold the mod order, the
    # profiles and the save backups, and an installer is the last thing
    # that should decide those are disposable.
    say "Settings and data were kept:"
    say "  ${XDG_CONFIG_HOME:-$HOME/.config}/$BIN_NAME"
    say "  $DATA_HOME/$BIN_NAME"
    exit 0
fi

# ------------------------------------------------------------------ install

[ "$(uname -s)" = "Linux" ] || die "this launcher is for Linux; found $(uname -s)"

case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    *)
        # Space Marine 2 runs through Proton, which is x86_64 only, so
        # there is no other architecture to build for today. Saying which
        # one was found beats a download that 404s.
        die "no build for $(uname -m) — only x86_64 is released"
        ;;
esac

for tool in tar sha256sum awk sort; do
    have "$tool" || die "$tool not found, but needed to install"
done

if [ -n "$wanted_version" ]; then
    version="${wanted_version#v}"
else
    step "looking up the latest release of $REPO"
    release_json="$(mktemp)"
    trap 'rm -f "$release_json"' EXIT
    download "$API_BASE/repos/$REPO/releases/latest" "$release_json" ||
        die "could not reach the release list — is there a network connection?"
    version="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' "$release_json" | head -n 1)"
    rm -f "$release_json"
    trap - EXIT
    [ -n "$version" ] || die "$REPO has no published release yet"
fi

present="$(installed_version)"
if [ -n "$present" ] && [ "$force" = "no" ]; then
    if [ "$present" = "$version" ]; then
        say "$DISPLAY_NAME $present is already installed. Use --force to reinstall."
        exit 0
    fi
    if [ "$(higher_version "$present" "$version")" = "$present" ]; then
        say "$present is already installed and newer than the release $version."
        say "Use --force to install $version anyway."
        exit 0
    fi
    step "updating $present → $version"
fi

archive="$BIN_NAME-$version-$arch-linux.tar.gz"
base_url="$DOWNLOAD_BASE/v$version"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

step "downloading $archive"
download "$base_url/$archive" "$work/$archive" ||
    die "$archive not found under $base_url"
download "$base_url/SHA256SUMS" "$work/SHA256SUMS" ||
    die "SHA256SUMS not found under $base_url — refusing to install unverified"

# The checksum is checked before anything is unpacked, let alone made
# executable. A truncated download and a tampered file look the same here,
# and both have to stop the installation rather than be repaired.
step "checking the checksum"
(
    cd "$work"
    grep -E "[[:space:]]\*?$archive\$" SHA256SUMS > expected.txt ||
        die "$archive is not listed in SHA256SUMS"
    sha256sum -c expected.txt >/dev/null 2>&1 ||
        die "checksum of $archive does not match — download discarded, nothing installed"
)

step "unpacking"
mkdir -p "$work/unpacked"
tar -xzf "$work/$archive" -C "$work/unpacked" --strip-components=1
[ -f "$work/unpacked/$BIN_NAME" ] || die "the archive does not contain $BIN_NAME"

mkdir -p "$BIN_DIR" "$DESKTOP_DIR" "$ICON_DIR"

# Written next to the target and then renamed over it. The rename is
# atomic, so a crash here leaves the previous version in place rather than
# a half-written file; and it replaces the directory entry rather than the
# running inode, so an open launcher keeps working until it is closed.
step "installing to $BIN_DIR/$BIN_NAME"
staged="$BIN_DIR/.$BIN_NAME.new.$$"
cp "$work/unpacked/$BIN_NAME" "$staged"
chmod 755 "$staged"
mv -f "$staged" "$BIN_DIR/$BIN_NAME"

if [ -f "$work/unpacked/$BIN_NAME.svg" ]; then
    cp "$work/unpacked/$BIN_NAME.svg" "$ICON_FILE"
fi

# The desktop entry points at the absolute path, not at the bare name: the
# menu does not read the shell's PATH, so an entry saying `Exec=lina-sm2`
# would silently do nothing on exactly the machines where the PATH hint
# below applies.
if [ -f "$work/unpacked/$BIN_NAME.desktop" ]; then
    sed "s|^Exec=.*|Exec=$BIN_DIR/$BIN_NAME|" \
        "$work/unpacked/$BIN_NAME.desktop" > "$DESKTOP_FILE"
    chmod 644 "$DESKTOP_FILE"
fi

refresh_desktop_caches

say ""
say "$DISPLAY_NAME $version installed."
say "  $BIN_DIR/$BIN_NAME"
# `|| true`, because under `set -e` a failing test at the end of an `&&`
# list aborts the script — and a missing optional file must not turn a
# finished installation into an error.
[ -f "$DESKTOP_FILE" ] && say "  $DESKTOP_FILE" || true
[ -f "$ICON_FILE" ] && say "  $ICON_FILE" || true
say ""

case ":$PATH:" in
    *":$BIN_DIR:"*)
        say "Start it from the application menu, or with: $BIN_NAME"
        ;;
    *)
        # Deliberately not written by this script. Which file is the right
        # one depends on the shell and on how the user has arranged their
        # configuration, and an installer editing shell startup files
        # behind their back is how those files end up broken.
        say "Start it from the application menu, or with: $BIN_DIR/$BIN_NAME"
        say ""
        say "$BIN_DIR is not in your PATH. To type '$BIN_NAME' instead, add"
        say "this line to your shell's startup file (~/.bashrc, ~/.zshrc, …):"
        say ""
        say "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        ;;
esac
