#!/bin/bash
# Compare two pepe binaries on the workloads that matter most, and fail if
# the second costs noticeably more CPU or memory than the first. This is
# the regression gate CI runs on release PRs (see .github/workflows/bench.yml).
#
#   bench/compare.sh BASELINE_BINARY CANDIDATE_BINARY [ROUNDS]
#
# Each workload is run ROUNDS times (3 by default) with the two binaries
# taking turns, and the best round of each is compared: the minimum is the
# measurement least disturbed by whatever else the machine was doing.
# Prints a Markdown table (also to $GITHUB_STEP_SUMMARY when set) and exits
# 1 if any workload's CPU per 1,000 requests grew by more than CPU_LIMIT
# (15%) or its peak memory by more than MEM_LIMIT (25%).
set -euo pipefail
cd "$(dirname "$0")/.."
base=${1:?baseline binary}
new=${2:?candidate binary}
rounds=${3:-3}
BASE_URL=${BASE:-http://127.0.0.1:8089}
TLS_URL=${TLS_BASE:-https://127.0.0.1:8090}
CPU_LIMIT=${CPU_LIMIT:-15}
MEM_LIMIT=${MEM_LIMIT:-25}

# name requests concurrency url [pepe args...]
workloads=(
  "tiny-c64|200000|64|$BASE_URL/|"
  "body16k-c64|50000|64|$BASE_URL/16k|"
  "slow20ms-c1000|50000|1000|$BASE_URL/slow?ms=20|"
  "tls-c64|100000|64|$TLS_URL/|-k"
)

# measure BINARY REQUESTS CONCURRENCY URL [args] -> "cpu_ms_per_1k rss_mb"
measure() {
  local bin=$1 n=$2 c=$3 url=$4; shift 4
  local line
  line=$(REQUESTS="$n" bench/measure.sh x "$bin" --json -n "$n" -c "$c" "$@" "$url")
  # label,wall,user,sys,cpu,per_k,rss
  echo "$line" | awk -F, '{print $6, $7}'
}

min() { printf '%s\n' "$@" | sort -n | head -1; }

table="| Workload | CPU ms / 1k, before | after | change | peak MB, before | after | change |\n|---|---|---|---|---|---|---|\n"
failed=0
for spec in "${workloads[@]}"; do
  IFS='|' read -r name n c url args <<< "$spec"
  base_cpu=(); new_cpu=(); base_mem=(); new_mem=()
  for ((r = 0; r < rounds; r++)); do
    # Alternate who goes first, so neither always runs on a warm or a busy machine
    if (( r % 2 == 0 )); then order=(base new); else order=(new base); fi
    for who in "${order[@]}"; do
      bin=${!who}
      read -r cpu mem < <(measure "$bin" "$n" "$c" "$url" ${args:+$args})
      if [[ $who == base ]]; then base_cpu+=("$cpu"); base_mem+=("$mem"); else new_cpu+=("$cpu"); new_mem+=("$mem"); fi
    done
  done
  bc=$(min "${base_cpu[@]}"); nc=$(min "${new_cpu[@]}")
  bm=$(min "${base_mem[@]}"); nm=$(min "${new_mem[@]}")
  cpu_pct=$(awk -v a="$bc" -v b="$nc" 'BEGIN {printf "%+.1f", (b-a)/a*100}')
  mem_pct=$(awk -v a="$bm" -v b="$nm" 'BEGIN {printf "%+.1f", (b-a)/a*100}')
  mark=""
  if awk -v p="$cpu_pct" -v l="$CPU_LIMIT" 'BEGIN {exit !(p > l)}'; then mark=" ❌ CPU"; failed=1; fi
  if awk -v p="$mem_pct" -v l="$MEM_LIMIT" 'BEGIN {exit !(p > l)}'; then mark="$mark ❌ memory"; failed=1; fi
  table+="| $name | $bc | $nc | ${cpu_pct}% | $bm | $nm | ${mem_pct}%$mark |\n"
  echo "$name: cpu $bc -> $nc ms/1k (${cpu_pct}%), peak $bm -> $nm MB (${mem_pct}%)$mark" >&2
done

printf "%b" "$table"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo "## Benchmark gate"
    echo
    echo "Best of $rounds rounds each, binaries taking turns. Limits: CPU +${CPU_LIMIT}%, memory +${MEM_LIMIT}%."
    echo
    printf "%b" "$table"
  } >> "$GITHUB_STEP_SUMMARY"
fi
if (( failed )); then
  echo "regression: the candidate costs more than the limits allow" >&2
  exit 1
fi
echo "no regression beyond the limits" >&2
