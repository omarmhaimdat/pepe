#!/bin/sh
# pepe's installer, served at https://pepe.mhaimdat.com/install.sh
#
#   curl -LsSf https://pepe.mhaimdat.com/install.sh | sh
#
# Runs the release installer (built by cargo-dist; it puts `pepe` in
# ~/.local/bin, or $XDG_BIN_HOME, and adds that to PATH), then has the new
# pepe set up tab completion and the man pages for your shell:
# `pepe completions --install`. Arguments go to the release installer, e.g.
# --no-modify-path. Set PEPE_NO_COMPLETIONS=1 to skip the completion step.
set -eu

installer="${PEPE_INSTALLER_URL:-https://pepe.mhaimdat.com/latest/pepe-installer.sh}"

if command -v curl >/dev/null 2>&1; then
  fetch() { curl --proto '=https' --tlsv1.2 -LsSf "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO- "$1"; }
else
  echo "pepe's installer needs curl or wget" >&2
  exit 1
fi

fetch "$installer" | sh -s -- "$@"

# Where the release installer puts the binary: the first of these that is
# set, as in dist-workspace.toml's install-path
if [ -n "${PEPE_NO_COMPLETIONS:-}" ]; then
  exit 0
fi
for dir in "${CARGO_DIST_FORCE_INSTALL_DIR:-}" "${XDG_BIN_HOME:-}" "$HOME/.local/bin"; do
  if [ -n "$dir" ] && [ -x "$dir/pepe" ]; then
    echo
    "$dir/pepe" completions --install || \
      echo "tab completion wasn't set up; run: pepe completions --install" >&2
    exit 0
  fi
done
echo "pepe is installed; run 'pepe completions --install' for tab completion" >&2
