#!/bin/sh
#
# Runs `install.sh` end to end against a release this script publishes
# itself, in a directory of its own. No network, no GitHub, nothing written
# outside a temporary directory.
#
# It can do that because the installer takes its two endpoints from
# `LINA_SM2_API_BASE` and `LINA_SM2_DOWNLOAD_BASE`, and `curl`/`wget` read
# `file://` URLs like any other. What is exercised is therefore the real
# script, not a copy of its logic: the checksum check, the atomic replace,
# the version comparison and the uninstall all run exactly as they will on
# a user's machine.
#
# `cargo test` runs this through `crates/app/tests/install_script.rs`, so
# it cannot quietly rot while the installer changes.

set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
INSTALLER="$ROOT/install.sh"
REPO="test-owner/test-repo"

tests=0
failures=0
current=""

pass() { printf 'ok    %s\n' "$current"; }

fail() {
    failures=$((failures + 1))
    printf 'FAIL  %s\n        %s\n' "$current" "$*" >&2
}

# --------------------------------------------------------------- the world

# A fresh, empty machine: a home directory and two empty endpoints.
new_machine() {
    sandbox="$(mktemp -d)"
    trash="$trash $sandbox"
    home="$sandbox/home"
    api="$sandbox/api"
    dl="$sandbox/dl"
    bin_dir="$home/.local/bin"
    desktop_file="$home/.local/share/applications/lina-sm2.desktop"
    icon_base="$home/.local/share/icons/hicolor"
    mkdir -p "$home" "$api" "$dl"
}

# The icon is a raster now, so it is one file per size rather than a
# single scalable one. These are the sizes `packaging/make-icons.py`
# writes and `install.sh` installs; the tests below check every one of
# them, because a loop that stops after the first size would pass just as
# happily.
ICON_SIZES="48 64 128 256"

icon_path() {
    printf '%s/%sx%s/apps/lina-sm2.png' "$icon_base" "$1" "$1"
}

# Publishes version $1 as the latest release: a tarball holding a stand-in
# binary that answers `--version` like the real one, the desktop entry and
# the icon this repository ships, plus the SHA256SUMS over it.
publish_release() {
    version="$1"
    name="lina-sm2-$version-x86_64-linux"

    mkdir -p "$api/repos/$REPO/releases" "$dl/v$version"
    printf '{"tag_name":"v%s","name":"Release %s"}\n' "$version" "$version" \
        > "$api/repos/$REPO/releases/latest"

    stage="$sandbox/stage"
    rm -rf "$stage"
    mkdir -p "$stage/$name"

    cat > "$stage/$name/lina-sm2" <<EOF
#!/bin/sh
if [ "\${1:-}" = "--version" ]; then
    echo "lina-sm2 $version"
fi
exit 0
EOF
    chmod 755 "$stage/$name/lina-sm2"
    cp "$ROOT/packaging/lina-sm2.desktop" "$stage/$name/"
    for size in $ICON_SIZES; do
        cp "$ROOT/packaging/icons/lina-sm2-$size.png" "$stage/$name/"
    done

    (cd "$stage" && tar -czf "$dl/v$version/$name.tar.gz" "$name")
    (cd "$dl/v$version" && sha256sum "$name.tar.gz" > SHA256SUMS)
}

# Runs the installer as if it were this machine's only user.
run_installer() {
    HOME="$home" \
        XDG_DATA_HOME="$home/.local/share" \
        XDG_CONFIG_HOME="$home/.config" \
        LINA_SM2_REPO="$REPO" \
        LINA_SM2_API_BASE="file://$api" \
        LINA_SM2_DOWNLOAD_BASE="file://$dl" \
        LINA_SM2_BIN_DIR="$bin_dir" \
        sh "$INSTALLER" "$@"
}

installed_version() {
    "$bin_dir/lina-sm2" --version 2>/dev/null | awk '{print $NF}'
}

# ---------------------------------------------------------------- the tests

# The plain case, and the one every other test builds on: three files in
# the three places the XDG layout puts them.
test_a_fresh_installation_places_binary_desktop_entry_and_icon() {
    new_machine
    publish_release 0.2.0

    run_installer > "$sandbox/out" 2>&1 || {
        fail "the installer exited non-zero: $(cat "$sandbox/out")"
        return
    }

    [ -x "$bin_dir/lina-sm2" ] || { fail "no executable at $bin_dir/lina-sm2"; return; }
    [ -f "$desktop_file" ] || { fail "no desktop entry at $desktop_file"; return; }
    for size in $ICON_SIZES; do
        [ -f "$(icon_path "$size")" ] || { fail "no icon at $(icon_path "$size")"; return; }
    done
    [ "$(installed_version)" = "0.2.0" ] || { fail "installed the wrong version"; return; }
    pass
}

# The menu does not read the shell's PATH. An entry left at `Exec=lina-sm2`
# would do nothing at all on exactly the machines where ~/.local/bin is not
# on the PATH — the ones the installer warns about.
test_the_desktop_entry_points_at_the_absolute_path() {
    new_machine
    publish_release 0.2.0
    run_installer > /dev/null 2>&1

    exec_line="$(grep '^Exec=' "$desktop_file")"
    [ "$exec_line" = "Exec=$bin_dir/lina-sm2" ] || {
        fail "expected Exec=$bin_dir/lina-sm2, found: $exec_line"
        return
    }
    # The window's app id has to stay untouched, or the running launcher
    # is not connected to this entry any more.
    grep -q '^StartupWMClass=lina-sm2$' "$desktop_file" || {
        fail "StartupWMClass was lost"
        return
    }
    pass
}

test_an_explicitly_requested_version_is_installed() {
    new_machine
    publish_release 0.1.0
    publish_release 0.3.0 # this one is "latest" from here on

    run_installer --version 0.1.0 > /dev/null 2>&1
    [ "$(installed_version)" = "0.1.0" ] || { fail "--version was ignored"; return; }
    pass
}

test_reinstalling_the_same_version_is_skipped() {
    new_machine
    publish_release 0.2.0
    run_installer > /dev/null 2>&1

    run_installer > "$sandbox/out" 2>&1 || {
        fail "a second run must succeed, not fail: $(cat "$sandbox/out")"
        return
    }
    grep -q "already installed" "$sandbox/out" || {
        fail "the second run said nothing about an existing installation"
        return
    }
    pass
}

test_an_older_installation_is_updated() {
    new_machine
    publish_release 0.1.0
    run_installer > /dev/null 2>&1

    publish_release 0.2.0
    run_installer > /dev/null 2>&1

    [ "$(installed_version)" = "0.2.0" ] || { fail "the update did not happen"; return; }
    pass
}

# The interesting direction: someone running a self-built newer version
# must not be pushed back onto an older release by re-running the one-liner.
test_a_newer_installation_is_kept_unless_forced() {
    new_machine
    publish_release 0.9.0
    run_installer > /dev/null 2>&1

    publish_release 0.10.0
    run_installer > /dev/null 2>&1
    [ "$(installed_version)" = "0.10.0" ] || {
        fail "0.10.0 must count as newer than 0.9.0, not older"
        return
    }

    publish_release 0.9.0
    run_installer > "$sandbox/out" 2>&1
    [ "$(installed_version)" = "0.10.0" ] || { fail "an older release overwrote a newer one"; return; }

    run_installer --force > /dev/null 2>&1
    [ "$(installed_version)" = "0.9.0" ] || { fail "--force did not install the older release"; return; }
    pass
}

# The one that matters most. A tampered or truncated download must stop the
# installation *before* anything is unpacked, and must leave whatever was
# installed before exactly as it was.
test_a_tampered_archive_is_refused_and_the_previous_version_survives() {
    new_machine
    publish_release 0.1.0
    run_installer > /dev/null 2>&1

    publish_release 0.2.0
    # Deliberately a *valid* tarball holding a different binary, not a file
    # of garbage: garbage would already fail at `tar`, and the test would
    # then pass whether the checksum is verified or not. Only a perfectly
    # unpackable archive whose bytes do not match SHA256SUMS proves that
    # the check itself is what stops the installation.
    swap="$sandbox/swap/lina-sm2-0.2.0-x86_64-linux"
    mkdir -p "$swap"
    printf '#!/bin/sh\necho "lina-sm2 6.6.6"\n' > "$swap/lina-sm2"
    chmod 755 "$swap/lina-sm2"
    cp "$ROOT/packaging/lina-sm2.desktop" "$swap/"
    for size in $ICON_SIZES; do
        cp "$ROOT/packaging/icons/lina-sm2-$size.png" "$swap/"
    done
    (cd "$sandbox/swap" && tar -czf "$dl/v0.2.0/lina-sm2-0.2.0-x86_64-linux.tar.gz" \
        "lina-sm2-0.2.0-x86_64-linux")

    if run_installer > "$sandbox/out" 2>&1; then
        fail "the installer accepted an archive that does not match SHA256SUMS"
        return
    fi
    grep -qi "checksum" "$sandbox/out" || {
        fail "the failure did not mention the checksum: $(cat "$sandbox/out")"
        return
    }
    [ "$(installed_version)" = "0.1.0" ] || {
        fail "the working installation was damaged by a failed update"
        return
    }
    pass
}

# Without SHA256SUMS there is nothing to check against, and "install it
# unverified" is not an option the script offers.
test_a_release_without_checksums_is_refused() {
    new_machine
    publish_release 0.2.0
    rm -f "$dl/v0.2.0/SHA256SUMS"

    if run_installer > "$sandbox/out" 2>&1; then
        fail "the installer accepted a release without SHA256SUMS"
        return
    fi
    [ -e "$bin_dir/lina-sm2" ] && { fail "something was installed anyway"; return; }
    pass
}

test_a_repository_without_a_release_fails_with_a_clear_message() {
    new_machine
    mkdir -p "$api/repos/$REPO/releases"
    printf '{"message":"Not Found"}\n' > "$api/repos/$REPO/releases/latest"

    if run_installer > "$sandbox/out" 2>&1; then
        fail "the installer claimed success without a release"
        return
    fi
    grep -q "no published release" "$sandbox/out" || {
        fail "unhelpful message: $(cat "$sandbox/out")"
        return
    }
    pass
}

# The uninstall removes what the installer put there and nothing else.
# Profiles and save backups are the whole reason this launcher exists; an
# uninstall that takes them with it is a data loss, not a cleanup.
test_uninstall_removes_the_three_files_and_keeps_settings_and_data() {
    new_machine
    publish_release 0.2.0
    run_installer > /dev/null 2>&1

    mkdir -p "$home/.config/lina-sm2" "$home/.local/share/lina-sm2/profiles"
    printf 'language = "de"\n' > "$home/.config/lina-sm2/settings.toml"
    printf 'a profile\n' > "$home/.local/share/lina-sm2/profiles/mine.json"

    run_installer --uninstall > "$sandbox/out" 2>&1 || {
        fail "the uninstall exited non-zero: $(cat "$sandbox/out")"
        return
    }

    [ -e "$bin_dir/lina-sm2" ] && { fail "the binary is still there"; return; }
    [ -e "$desktop_file" ] && { fail "the desktop entry is still there"; return; }
    for size in $ICON_SIZES; do
        [ -e "$(icon_path "$size")" ] && { fail "$(icon_path "$size") is still there"; return; }
    done
    [ -f "$home/.config/lina-sm2/settings.toml" ] || { fail "the settings were deleted"; return; }
    [ -f "$home/.local/share/lina-sm2/profiles/mine.json" ] || { fail "a profile was deleted"; return; }
    pass
}

test_uninstalling_nothing_is_not_an_error() {
    new_machine

    run_installer --uninstall > "$sandbox/out" 2>&1 || {
        fail "an uninstall with nothing installed must not fail"
        return
    }
    grep -q "not installed" "$sandbox/out" || { fail "it said nothing useful"; return; }
    pass
}

test_an_unknown_option_is_rejected() {
    new_machine

    if run_installer --wat > "$sandbox/out" 2>&1; then
        fail "an unknown option was accepted"
        return
    fi
    grep -q "unknown option" "$sandbox/out" || { fail "unhelpful message"; return; }
    pass
}

# ------------------------------------------------------------------ runner

trash=""
cleanup() {
    for dir in $trash; do rm -rf "$dir"; done
}
trap cleanup EXIT

[ -f "$INSTALLER" ] || {
    printf 'install.sh not found at %s\n' "$INSTALLER" >&2
    exit 1
}
command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 || {
    printf 'neither curl nor wget available — cannot test the installer\n' >&2
    exit 1
}

for test_case in $(grep -o '^test_[a-z_]*' "$0"); do
    current="$test_case"
    tests=$((tests + 1))
    "$test_case"
done

printf '\n%s tests, %s failures\n' "$tests" "$failures"
[ "$failures" -eq 0 ]
