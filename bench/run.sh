#!/bin/bash
# The benchmark suite behind bench/README.md.
#
#   go run bench/server.go &          # the target, in another shell
#   bench/run.sh [PEPE_BINARY]        # defaults to target/release/pepe
#
# Every workload is run with pepe (--json, no screen), and where a comparable
# invocation exists, with oha and vegeta too. Each line of the CSV on stdout
# is: tool, workload, requests, wall s, user s, sys s, cpu s, cpu ms per
# 1,000 requests, peak RSS MB, requests per second as the tool reported it.
set -u
cd "$(dirname "$0")/.."
PEPE=${1:-target/release/pepe}
BASE=${BASE:-http://127.0.0.1:8089}
TLS_BASE=${TLS_BASE:-https://127.0.0.1:8090}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# measure LABEL COMMAND...  -> sets wall user sys rss
measure() {
  local out="$tmp/time"
  if [[ "$(uname)" == "Darwin" ]]; then
    /usr/bin/time -l "$@" >"$tmp/stdout" 2>"$out"
    wall=$(awk '/real/ {print $1}' "$out")
    user=$(awk '/real/ {print $3}' "$out")
    sys=$(awk '/real/ {print $5}' "$out")
    rss=$(awk '/maximum resident set size/ {printf "%.1f", $1/1048576}' "$out")
  else
    /usr/bin/time -f '%e %U %S %M' "$@" >"$tmp/stdout" 2>"$out"
    read -r wall user sys kb < <(tail -1 "$out")
    rss=$(awk -v kb="$kb" 'BEGIN {printf "%.1f", kb/1024}')
  fi
}

row() { # tool workload requests rps
  local cpu per_k
  cpu=$(awk -v u="$user" -v s="$sys" 'BEGIN {printf "%.3f", u+s}')
  per_k=$(awk -v c="$cpu" -v n="$3" 'BEGIN {if (n>0) printf "%.2f", c*1000*1000/n; else print ""}')
  echo "$1,$2,$3,$wall,$user,$sys,$cpu,$per_k,$rss,$4"
}

# workload NAME N C PATH [extra pepe args...]
# A path starting with "tls" goes to the HTTPS listener, certificate unchecked.
workload() {
  local name=$1 n=$2 c=$3 path=$4; shift 4
  # Plain strings, not arrays: macOS's bash 3.2 trips on empty arrays with set -u
  local url="$BASE$path" pepe_tls="" oha_tls="" vegeta_tls=""
  if [[ $path == tls* ]]; then
    url="$TLS_BASE${path#tls}"
    pepe_tls="-k"; oha_tls="--insecure"; vegeta_tls="-insecure"
  fi

  measure "$PEPE" --json -n "$n" -c "$c" $pepe_tls "$@" "$url"
  local got rps
  got=$(jq -r '.summary.total_requests' "$tmp/stdout")
  rps=$(jq -r '.summary.requests_per_second | floor' "$tmp/stdout")
  row pepe "$name" "$got" "$rps"

  if command -v oha >/dev/null; then
    measure oha --no-tui -j -n "$n" -c "$c" $oha_tls "$url"
    got=$(jq -r '[.statusCodeDistribution[]] | add' "$tmp/stdout")
    rps=$(jq -r '.summary.requestsPerSec | floor' "$tmp/stdout")
    row oha "$name" "$got" "$rps"
  fi

  if command -v vegeta >/dev/null && [[ -z "${NO_VEGETA:-}" ]]; then
    # vegeta runs for a time, not a count: as fast as C workers can go
    local secs=${VEGETA_SECS:-5}
    measure sh -c "echo 'GET $url' | vegeta attack -rate=0 -max-workers=$c -duration=${secs}s -timeout=30s $vegeta_tls | vegeta report -type=json"
    got=$(jq -r '.requests' "$tmp/stdout")
    rps=$(jq -r '.rate | floor' "$tmp/stdout")
    row vegeta "$name" "$got" "$rps"
  fi
}

echo "tool,workload,requests,wall_s,user_s,sys_s,cpu_s,cpu_ms_per_1k,peak_rss_mb,rps"
workload tiny-c64      200000  64   /
workload tiny-c256     200000  256  /
workload tiny-c1000    200000  1000 /
workload json-c64      200000  64   /json
workload body16k-c64   50000   64   /16k
workload body256k-c16  5000    16   /256k
workload slow20ms-c1000 50000  1000 '/slow?ms=20'
workload status503-c64 100000  64   /status/503
workload tls-c64       100000  64   tls/
workload tls16k-c64    30000   64   tls/16k
NO_VEGETA=1 workload post-c64 200000 64 / -m POST -d '{"key":"value"}' -H 'Content-Type: application/json'
