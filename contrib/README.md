# Shell completions and man pages

Generated from pepe's command definition, so they always match the
binary's flags. Every release archive carries this directory; Homebrew
installs it under `$(brew --prefix)/share/pepe/`.

To regenerate after changing a flag: `UPDATE_CONTRIB=1 cargo test contrib`.
`cargo test` fails when these files are out of date.

## Completions

| Shell | File | Where to put it |
| --- | --- | --- |
| bash | `completions/pepe.bash` | `~/.local/share/bash-completion/completions/pepe`, or `source` it from `~/.bashrc` |
| zsh | `completions/_pepe` | A directory on `$fpath`, e.g. `~/.zsh/completions/`, then `compinit` |
| fish | `completions/pepe.fish` | `~/.config/fish/completions/` |
| PowerShell | `completions/_pepe.ps1` | Dot-source it from your `$PROFILE` |

For example, with the archive unpacked in the current directory:

```bash
# bash
mkdir -p ~/.local/share/bash-completion/completions
cp contrib/completions/pepe.bash ~/.local/share/bash-completion/completions/pepe

# zsh
mkdir -p ~/.zsh/completions && cp contrib/completions/_pepe ~/.zsh/completions/
# then in ~/.zshrc, before compinit: fpath=(~/.zsh/completions $fpath)

# fish
cp contrib/completions/pepe.fish ~/.config/fish/completions/
```

With a Homebrew install, the files are at `$(brew --prefix)/share/pepe/completions/`.

## Man pages

`man/pepe.1` covers the command, the keys of every screen, examples and
environment variables; `pepe-ramp.1`, `pepe-api.1` and
`pepe-self-update.1` cover the subcommands.

```bash
man ./contrib/man/pepe.1                       # read it in place
sudo cp contrib/man/*.1 /usr/local/share/man/man1/   # or install it
```
