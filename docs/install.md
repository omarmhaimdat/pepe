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

Homebrew and Nix installs update with `brew upgrade pepe` and `nix profile upgrade pepe`. Set `PEPE_NO_UPDATE_CHECK=1` to turn the daily check off; it is off in CI already. `PEPE_GITHUB_TOKEN` gives `self-update` a GitHub token, for forks or rate-limited CI, and `PEPE_CACHE_DIR` is where the check keeps its answer.

## Tab completion and man pages

Bash, zsh, fish and PowerShell completions and `man pepe` (and `man pepe-ping`, `pepe-ramp`, …) come with the installer and the Homebrew formula. Installed another way:

```bash
pepe completions --install            # for the shell you're in
pepe completions --install --dry-run  # see what it would change first
pepe completions zsh                  # print the script, for packagers and dotfiles
```

`--install` writes the completion file and the man pages under `~/.local/share` (fish: `~/.config/fish/completions`) and appends what the shell needs to its startup file, once, marked with a comment. It is safe to run again; `pepe self-update` runs it for you after an update.

## Themes and colour

pepe draws in its own palette on terminals that say they can show true colour, and in the terminal's own 256-colour palette otherwise. `PEPE_THEME=light` or `dark` picks the palette for the terminal's background when `COLORFGBG` doesn't say; `NO_COLOR=1` draws without colour, with selections in reverse video, the heatmap in shades and the verdict uncoloured.
