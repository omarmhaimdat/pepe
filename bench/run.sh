#!/bin/bash
# The benchmark suite behind bench/README.md.
#
#   cargo run --release --manifest-path bench/server/Cargo.toml &   # the target
#   bench/run.sh [PEPE_BINARY]        # defaults to target/release/pepe
#   MILLIONS=10 bench/run.sh          # and a ten-million-request run at the end
#   NO_OHA=1 NO_VEGETA=1 NO_WRK=1 NO_K6=1 bench/run.sh   # pepe alone
#
# Every workload is run with pepe (--json, no screen), and where a comparable
# invocation exists, with oha, vegeta, wrk and k6 too, whichever of them are
# installed. Each line of the CSV on stdout is: tool, workload, requests,
# wall s, user s, sys s, cpu s, cpu ms per 1,000 requests, peak RSS MB,
# requests per second as the tool reported it.
set -u
cd "$(dirname "$0")/.."
PEPE=${1:-target/release/pepe}
BASE=${BASE:-http://127.0.0.1:8089}
TLS_BASE=${TLS_BASE:-https://127.0.0.1:8090}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# measure LABEL COMMAND...  -> sets wall user sys rss; the tool's own
# stdout and stderr land in $tmp/stdout and $tmp/stderr
measure() {
  local out="$tmp/time"
  if [[ "$(uname)" == "Darwin" ]]; then
    # macOS's time has no -o: its report shares stderr with the tool's
    /usr/bin/time -l "$@" >"$tmp/stdout" 2>"$out" || true
    cp "$out" "$tmp/stderr"
    wall=$(awk '/real/ {print $1}' "$out")
    user=$(awk '/real/ {print $3}' "$out")
    sys=$(awk '/real/ {print $5}' "$out")
    rss=$(awk '/maximum resident set size/ {printf "%.1f", $1/1048576}' "$out")
  else
    /usr/bin/time -o "$out" -f '%e %U %S %M' "$@" >"$tmp/stdout" 2>"$tmp/stderr" || true
    read -r wall user sys kb < <(tail -1 "$out")
    rss=$(awk -v kb="$kb" 'BEGIN {printf "%.1f", kb/1024}')
  fi
}

# Say why a tool produced no result, instead of an empty row in silence
complain() { # tool count
  if [[ -z "$2" || "$2" == "null" ]]; then
    echo "$1 gave no result; what it said:" >&2
    head -20 "$tmp/stderr" >&2
    head -5 "$tmp/stdout" >&2
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
  got=$(jq -r '.summary.total_requests' "$tmp/stdout" 2>/dev/null)
  rps=$(jq -r '.summary.requests_per_second | floor' "$tmp/stdout" 2>/dev/null)
  complain pepe "$got"
  row pepe "$name" "$got" "$rps"

  if command -v oha >/dev/null && [[ -z "${NO_OHA:-}" ]]; then
    measure oha --no-tui $OHA_JSON -n "$n" -c "$c" $oha_tls "$url"
    got=$(jq -r '[.statusCodeDistribution[]] | add' "$tmp/stdout" 2>/dev/null)
    rps=$(jq -r '.summary.requestsPerSec | floor' "$tmp/stdout" 2>/dev/null)
    complain oha "$got"
    row oha "$name" "$got" "$rps"
  fi

  if command -v vegeta >/dev/null && [[ -z "${NO_VEGETA:-}" ]]; then
    # vegeta runs for a time, not a count: as fast as C workers can go
    local secs=${VEGETA_SECS:-5}
    measure sh -c "echo 'GET $url' | vegeta attack -rate=0 -max-workers=$c -duration=${secs}s -timeout=30s $vegeta_tls | vegeta report -type=json"
    got=$(jq -r '.requests' "$tmp/stdout" 2>/dev/null)
    rps=$(jq -r '.rate | floor' "$tmp/stdout" 2>/dev/null)
    complain vegeta "$got"
    row vegeta "$name" "$got" "$rps"
  fi

  # wrk runs for a time too, on one thread to be read against pepe's one
  if command -v wrk >/dev/null && [[ -z "${NO_WRK:-}" ]]; then
    local secs=${WRK_SECS:-5}
    measure wrk -t1 -c "$c" -d "${secs}s" --timeout 30s "$url"
    got=$(awk '/requests in/ {print $1}' "$tmp/stdout")
    rps=$(awk '/Requests\/sec/ {printf "%d", $2}' "$tmp/stdout")
    complain wrk "$got"
    row wrk "$name" "$got" "$rps"
  fi

  # k6 runs a script: one that does nothing but the request
  if command -v k6 >/dev/null && [[ -z "${NO_K6:-}" ]]; then
    echo "import http from 'k6/http'; export default function () { http.get(__ENV.URL, { responseType: 'none' }); }" > "$tmp/k6.js"
    measure k6 run --quiet --no-usage-report --insecure-skip-tls-verify --vus "$c" --iterations "$n" \
      --summary-export "$tmp/k6.json" -e URL="$url" "$tmp/k6.js"
    got=$(jq -r '.metrics.http_reqs.count' "$tmp/k6.json" 2>/dev/null)
    rps=$(jq -r '.metrics.http_reqs.rate | floor' "$tmp/k6.json" 2>/dev/null)
    complain k6 "$got"
    row k6 "$name" "$got" "$rps"
  fi
}

# oha's JSON flag: --output-format json since 1.5, -j before
OHA_JSON="-j"
if command -v oha >/dev/null && oha --help 2>/dev/null | grep -q -- "--output-format"; then
  OHA_JSON="--output-format json"
fi

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
NO_VEGETA=1 NO_WRK=1 NO_K6=1 workload post-c64 200000 64 / -m POST -d '{"key":"value"}' -H 'Content-Type: application/json'
# Long enough that anything growing with the number of requests shows
workload million-c64   1000000 64   /
if [[ -n "${MILLIONS:-}" ]]; then
  workload "${MILLIONS}m-c256" $((MILLIONS * 1000000)) 256 /
fi
