#!/bin/bash
# Measure one load-generator run: wall time, CPU time and peak memory.
#
#   bench/measure.sh LABEL COMMAND...
#
# Prints one CSV line: label, wall seconds, user CPU seconds, system CPU
# seconds, CPU seconds per 1,000 requests (when REQUESTS is set), peak RSS in
# MB. Uses /usr/bin/time, so it works for any binary.
set -u
label=$1; shift
out=$(mktemp)
if [[ "$(uname)" == "Darwin" ]]; then
  /usr/bin/time -l "$@" >/dev/null 2>"$out"
  wall=$(awk '/real/ {print $1}' "$out")
  user=$(awk '/real/ {print $3}' "$out")
  sys=$(awk '/real/ {print $5}' "$out")
  rss=$(awk '/maximum resident set size/ {printf "%.1f", $1/1048576}' "$out")
else
  /usr/bin/time -f '%e %U %S %M' "$@" >/dev/null 2>"$out"
  read -r wall user sys kb < <(tail -1 "$out")
  rss=$(awk -v kb="$kb" 'BEGIN {printf "%.1f", kb/1024}')
fi
rm -f "$out"
cpu=$(awk -v u="$user" -v s="$sys" 'BEGIN {printf "%.3f", u+s}')
per_k=""
if [[ -n "${REQUESTS:-}" ]]; then
  per_k=$(awk -v c="$cpu" -v n="$REQUESTS" 'BEGIN {printf "%.3f", c*1000/n}')
fi
echo "$label,$wall,$user,$sys,$cpu,$per_k,$rss"
