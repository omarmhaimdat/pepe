#!/bin/bash
# Re-record the README's GIFs with vhs (https://github.com/charmbracelet/vhs).
#
#   cargo build --release
#   assets/record.sh [tape...]      # all of assets/tapes/*.tape by default
#
# Starts its own target (bench/server on port 8099, answering in 5 ms so
# the runs look like a real service rather than a loopback), records, and
# stops it.
set -euo pipefail
cd "$(dirname "$0")/.."
command -v vhs >/dev/null || { echo "vhs is needed: brew install vhs"; exit 1; }
test -x target/release/pepe || { echo "build first: cargo build --release"; exit 1; }
cargo build --quiet --release --manifest-path bench/server/Cargo.toml
bench/server/target/release/bench-server --addr 127.0.0.1:8099 --tls-addr "" --latency 5ms >/dev/null 2>&1 &
server=$!
trap 'kill $server 2>/dev/null' EXIT
for i in $(seq 1 100); do curl -sf http://127.0.0.1:8099/ >/dev/null && break; sleep 0.1; done
curl -sf http://127.0.0.1:8099/ >/dev/null || { echo "the target didn't start"; exit 1; }
tapes=("$@")
[ ${#tapes[@]} -gt 0 ] || tapes=(assets/tapes/*.tape)
for tape in "${tapes[@]}"; do
  echo "recording $tape"
  vhs "$tape"
done
ls -la assets/*.gif
