#!/usr/bin/env bash
# What `pepe logs --serve` costs: pepe's CPU, the tick, and the bytes each
# viewer gets, with nobody watching, one viewer, ten, and one on the log tab.
#
#   bench/logs-serve.sh 400 30      # 400 requests a second, 30 s a phase
#
# Needs a release build (cargo build --release). Prints a CSV on stdout.
set -euo pipefail
RATE=${1:-400}; PHASE=${2:-30}
PEPE=${PEPE:-./target/release/pepe}
DIR=$(mktemp -d); PORT=$((20000 + RANDOM % 20000))
trap 'kill $(jobs -p) 2>/dev/null; rm -rf "$DIR"' EXIT
: > "$DIR/access.log"; : > "$DIR/error.log"
python3 -I bench/gen-live-logs.py "$DIR" "$RATE" &
until [ "$(wc -l < "$DIR/access.log")" -gt $((RATE * 900)) ]; do sleep 1; done
PEPE_NO_UPDATE_CHECK=1 "$PEPE" logs "$DIR/access.log" "$DIR/error.log" --since 30m \
  --serve "127.0.0.1:$PORT" > /dev/null 2> "$DIR/err" &
PID=$!
until grep -q token= "$DIR/err" 2>/dev/null; do sleep .2; done
TOKEN=$(grep -o 'token=[0-9a-f]*' "$DIR/err" | cut -d= -f2)
AUTH="Authorization: Bearer $TOKEN"; BASE="http://127.0.0.1:$PORT"
sleep 5   # the history read
cpu() { ps -o cputime= -p "$PID" | awk -F: '{ s = 0; for (i = 1; i <= NF; i++) s = s * 60 + $i; print s }'; }
rss() { ps -o rss= -p "$PID" | awk '{ print int($1 / 1024) }'; }
echo "phase,rate,seconds,cpu_ms_per_s,rss_mb,viewers,bytes_per_viewer_per_s,tick_us,locked_us,ticks"
phase() {  # name, viewers, lines
  local name=$1 n=$2 lines=$3; pids=()
  for ((i = 1; i <= n; i++)); do
    curl -sN -H "$AUTH" "$BASE/api/stream" -o "$DIR/v$i" & pids+=($!)
  done
  if [ "$lines" = 1 ]; then
    ( after=0; while :; do
        curl -s -H "$AUTH" -H 'Accept-Encoding: gzip' "$BASE/api/lines?after=$after" -o "$DIR/l.raw"
        cat "$DIR/l.raw" >> "$DIR/lines"   # the bytes as sent, gzip'd
        after=$(python3 -c 'import gzip,json,sys; b=open(sys.argv[1],"rb").read(); b=gzip.decompress(b) if b[:2]==b"\x1f\x8b" else b; print(json.loads(b)["last"])' "$DIR/l.raw"); sleep 1
      done ) & pids+=($!)
  fi
  sleep 3   # past the first, catch-up event
  local b0=0 b1=0; for ((i = 1; i <= n; i++)); do b0=$((b0 + $(wc -c < "$DIR/v$i"))); done
  [ "$lines" = 1 ] && b0=$((b0 + $(wc -c < "$DIR/lines")))
  local c0; c0=$(cpu)
  sleep "$PHASE"
  local c1; c1=$(cpu)
  for ((i = 1; i <= n; i++)); do b1=$((b1 + $(wc -c < "$DIR/v$i"))); done
  [ "$lines" = 1 ] && b1=$((b1 + $(wc -c < "$DIR/lines")))
  local cost; cost=$(curl -s -H "$AUTH" "$BASE/api/cost")
  local tick lock; tick=$(printf %s "$cost" | python3 -c 'import json,sys; print(json.load(sys.stdin)["tick_us"])')
  lock=$(printf %s "$cost" | python3 -c 'import json,sys; print(json.load(sys.stdin)["locked_us"])')
  local ticks; ticks=$(printf %s "$cost" | python3 -c 'import json,sys; print(json.load(sys.stdin)["ticks"])')
  if [ ${#pids[@]} -gt 0 ]; then kill "${pids[@]}" 2>/dev/null || true; wait "${pids[@]}" 2>/dev/null || true; fi
  rm -f "$DIR"/v* "$DIR/lines"
  local per=0; [ "$n" -gt 0 ] && per=$(( (b1 - b0) / n / PHASE ))
  echo "$name,$RATE,$PHASE,$(awk -v a="$c0" -v b="$c1" -v p="$PHASE" 'BEGIN { printf "%.1f", (b - a) * 1000 / p }'),$(rss),$n,$per,$tick,$lock,$ticks"
  sleep 2
}
phase nobody 0 0
phase one 1 0
phase ten 10 0
phase "one on the log tab" 1 1
