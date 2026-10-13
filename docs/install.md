# Install

One binary, a few megabytes, for macOS (Apple Silicon and Intel), Linux (x86_64 and ARM64, statically linked) and Windows (x86_64). Every way below gives the same `pepe`.

## The installer

macOS and Linux:

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh
```

Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://pepe.mhaimdat.com/install.ps1 | iex"
```

The installer puts `pepe` in `~/.local/bin` (or where `CARGO_HOME`/bin would be), adds it to the shell's path if it isn't there, and sets up tab completion and the man pages for the shell you're in.

## Homebrew

macOS and Linux:

```bash
brew install omarmhaimdat/pepe/pepe
```

The formula puts the completions and man pages where Homebrew activates them.

## npm

```bash
npm install -g pepe-http                  # install it: the command is `pepe`
npx pepe-http -n 100 https://example.com  # try it without installing
```

The package is `pepe-http` (`pepe-cli` was taken); it downloads the binary for your platform when it installs.

## PyPI

```bash
pipx install pepe-http   # or: pip install pepe-http
```

The same name and the same `pepe`. The package is a few lines of Python with no dependencies: the first run downloads the release of the same version for your platform from GitHub, checks its SHA-256, keeps it in your cache directory (`~/.cache/pepe-http`, `~/Library/Caches/pepe-http` on macOS, `%LOCALAPPDATA%\pepe-http\Cache` on Windows; `PEPE_HTTP_CACHE_DIR` moves it) and runs it; every later run only runs it.

## Nix

```bash
nix run github:omarmhaimdat/pepe -- https://example.com   # try it without installing
nix profile install github:omarmhaimdat/pepe              # install it
```

## Docker

```bash
docker run --rm -it ghcr.io/omarmhaimdat/pepe -z 30s -c 50 https://example.com   # the dashboard needs -it
docker run --rm ghcr.io/omarmhaimdat/pepe --json -n 1000 https://example.com     # for scripts
```

An empty image with the static binary in it, for `linux/amd64` and `linux/arm64`, with `:latest` and `:<version>` tags. `docker build -t pepe .` in a checkout builds the same from source.

## Prebuilt binaries

Every [release](https://github.com/omarmhaimdat/pepe/releases) ships binaries for each platform with SHA-256 checksums and signed build provenance:

```bash
gh attestation verify pepe-x86_64-unknown-linux-musl.tar.xz --repo omarmhaimdat/pepe
```

Each archive also carries the shell completions, the man pages and the JSON Schema of each report (`schema/`). `pepe completions --install` puts the completions and man pages in place for your shell; `--dry-run` shows what it would change.

## From source

```bash
cargo install --locked --git https://github.com/omarmhaimdat/pepe
```

Rust 1.85 or newer.

## Staying up to date

pepe looks for a newer release once a day and says so when a run ends, with what changed and the command that updates your copy. Installer-based installs update themselves:

```bash
pepe self-update            # install the latest release
pepe self-update --check    # only say whether there is one (exit code 1 if so)
```

Homebrew and Nix installs update with `brew upgrade pepe` and `nix profile upgrade pepe`, npm and PyPI installs with `npm update -g pepe-http` and `pipx upgrade pepe-http` (each version of those packages runs the pepe of that version). Set `PEPE_NO_UPDATE_CHECK=1` to turn the daily check off; it is off in CI already. `PEPE_GITHUB_TOKEN` gives `self-update` a GitHub token, for forks or rate-limited CI, and `PEPE_CACHE_DIR` is where the check keeps its answer.

## Tab completion and man pages

Bash, zsh, fish and PowerShell completions and `man pepe` (and `man pepe-ping`, `pepe-ramp`, …) come with the installer and the Homebrew formula. Installed another way:

```bash
pepe completions --install            # for the shell you're in
pepe completions --install --dry-run  # see what it would change first
pepe completions zsh                  # print the script, for packagers and dotfiles
```

`--install` writes the completion file and the man pages under `~/.local/share` (fish: `~/.config/fish/completions`) and appends what the shell needs to its startup file, once, marked with a comment. It is safe to run again; `pepe self-update` runs it for you after an update.

## Themes and colour

pepe draws in its own palette on terminals that say they can show true colour, and in the terminal's own 256-colour palette otherwise. `F2` on any screen opens the [settings](#settings), where `←` `→` on the first line step through the themes, each drawn as it's picked and kept for next time:

| Theme | |
| --- | --- |
| `pepe` | pepe's own: warm darks, one ember accent |
| `nord`, `dracula`, `gruvbox`, `tokyo-night`, `catppuccin` | The same screens in those palettes, in true colour |
| `paper` | True colour on a light ground |
| `terminal` | The terminal's own background and 256 colours |
| `light` | The 256 colours chosen for a light background |
| `none` | No colour: selections in reverse video, the heatmap in shades, the verdict uncoloured |

`PEPE_THEME=nord` names one for a shell or a single run, and wins over the saved theme when pepe starts. With neither, `COLORFGBG` says whether the background is light and `COLORTERM` whether the terminal has true colour. `NO_COLOR=1` is `none`, whatever else is set.

## Settings

`F2` (or `ctrl-o`) on any screen opens the settings for this machine: the theme, the defaults a run starts from (requests, duration, concurrency, timeout, threads, rate, warm-up, user agent, proxy, keep-alive, compression, redirects, TLS verification), a ramp's steps, the [guardrails](agents.md#guardrails), and whether pepe looks for new releases. `↑` `↓` move, `enter` types a value, `←` `→` change a choice, `del` goes back to pepe's own, `esc` closes. Every change is written as it's made, to `~/.config/pepe/settings.toml` (`$XDG_CONFIG_HOME/pepe`; `%APPDATA%\pepe` on Windows; `PEPE_CONFIG_DIR` names another directory):

```toml
theme = "nord"
update-check = false

[defaults]             # what a pepe.toml can say, for every run on this machine
concurrency = 50
timeout = 5
max-rate = 500
```

The theme changes at once; the defaults are read when pepe starts. Flags win over a `pepe.toml`, which wins over these defaults, which win over pepe's own. The caps are the exception: where a `pepe.toml` and the settings both set `max-requests`, `max-rate` or `max-concurrency`, the lower one holds, so a file in a directory can't loosen what the machine is held to. They apply in every mode, with `--json` and under `pepe mcp` too.
