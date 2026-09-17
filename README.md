# LiNa SM2 - Mod Launcher

A mod launcher for **Warhammer 40,000: Space Marine 2** on Linux — Steam,
Proton, and a savegame directory that Steam Cloud can overwrite at any
moment. "LiNa" is short for *Linux native*.

It manages the mod load order, keeps profiles of it, makes verified
backups of your savegames before anything touches them, and starts the
game with or without mods and with or without EAC.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/Carlos17Kopra/linasm2-modloader/main/install.sh | sh
```

No root, no package manager. It puts three files under your home
directory and nothing else:

| File | Purpose |
| --- | --- |
| `~/.local/bin/lina-sm2` | the launcher itself (GUI and CLI in one binary) |
| `~/.local/share/applications/lina-sm2.desktop` | entry in the application menu |
| `~/.local/share/icons/hicolor/scalable/apps/lina-sm2.svg` | its icon |

The released binary is checked against the `SHA256SUMS` of the same
release before anything is unpacked; a mismatch stops the installation and
leaves an existing one untouched.

Running the same line again updates an existing installation, and says so
and stops if there is nothing newer to install.

> Piping a script from the internet into a shell means trusting whoever
> controls that URL. The script is short and does nothing clever — if you
> would rather read it first:
> `curl -fsSL .../install.sh -o install.sh && less install.sh && sh install.sh`

### Options

```sh
sh install.sh --version 0.2.0   # install a specific release
sh install.sh --force           # reinstall, or go back to an older release
sh install.sh --uninstall       # remove the three files above
```

`--uninstall` keeps your settings, profiles and savegame backups
(`~/.config/lina-sm2`, `~/.local/share/lina-sm2`). Delete those by hand if
you really want them gone.

### If `lina-sm2` is not found afterwards

`~/.local/bin` is not on your `PATH`. The application menu entry works
regardless, because it points at the absolute path. To use the name in a
shell, add this to `~/.bashrc` or `~/.zshrc`:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

The installer deliberately does not edit those files for you.

## Requirements

- Linux on x86_64 — the same architecture Proton needs
- Space Marine 2 installed through Steam and **started at least once**, so
  that its Proton prefix exists
- `curl` or `wget`, and `tar` — present on every desktop distribution

## Build from source

Needs Rust 1.95 or newer (`rustup`; almost no distribution ships it yet):

```sh
git clone https://github.com/Carlos17Kopra/linasm2-modloader.git
cd linasm2-modloader
cargo build --release      # target/release/lina-sm2
cargo test                 # the full suite, including the installer's
```

## Usage

Started without arguments it opens the graphical interface. With
arguments the very same binary is a command line tool:

```sh
lina-sm2 list              # every mod in load order
lina-sm2 enable <mod.pak>
lina-sm2 order a.pak b.pak # set the load order
lina-sm2 save backup       # back up the savegames
lina-sm2 play              # start the game with the current mods
lina-sm2 paths             # what was detected where
lina-sm2 --help
```

English and German are both built in; `lina-sm2 lang de` switches, and the
graphical interface has the same choice in its settings.

## License

MIT — see [LICENSE](LICENSE).
