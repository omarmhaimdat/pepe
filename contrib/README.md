# Shell completions and man pages

Tab completion for bash, zsh, fish and PowerShell, and man pages for
`pepe` and its subcommands. They are generated from pepe's command
definition (`src/contrib.rs`), so they always match the binary's flags;
`cargo test` fails when these files are out of date, and
`UPDATE_CONTRIB=1 cargo test contrib` regenerates them.

## You shouldn't need this directory

- **Installed with the install script or `pepe self-update`**: completion
  and the man pages are set up for your shell as part of the install.
- **Installed with Homebrew**: the formula puts them where Homebrew
  activates them. For zsh that is `$(brew --prefix)/share/zsh/site-functions`,
  which Homebrew's own setup instructions add to `fpath`.
- **Anything else** (cargo, Nix, a downloaded archive):

  ```bash
  pepe completions --install            # for the shell you're in
  pepe completions --install --dry-run  # see what it would change first
  ```

  It writes the completion file and the man pages under `~/.local/share`
  (fish: `~/.config/fish/completions`), and appends what the shell needs
  to its startup file, once, marked with a comment. It is safe to run
  again; `pepe self-update` runs it for you after an update.

`pepe completions zsh` (or bash, fish, powershell) prints the script for
packagers and dotfile setups.

## By hand

The same files, if you'd rather place them yourself. They are in every
release archive under `completions/` and `man/`, and here under `contrib/`.

| Shell | File | Where it goes |
| --- | --- | --- |
| bash | `completions/pepe.bash` | `~/.local/share/bash-completion/completions/pepe`, or `source` it from `~/.bashrc` |
| zsh | `completions/_pepe` | A directory on `$fpath`, before `compinit` runs |
| fish | `completions/pepe.fish` | `~/.config/fish/completions/` |
| PowerShell | `completions/_pepe.ps1` | Dot-source it from `$PROFILE` |

Man pages: `man ./man/pepe.1` reads one in place; `~/.local/share/man/man1/`
or `/usr/local/share/man/man1/` installs them. `pepe.1` has the keys of
every screen, examples and the environment variables, which `--help`
doesn't.
