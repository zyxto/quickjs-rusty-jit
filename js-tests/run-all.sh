#!/usr/bin/env sh

# Runs every js-tests/*.js file with Node, Bun, and this project's qjs binary.
# Appends all results to the same log file on every execution.
#
# Optional env vars:
#   LOG_FILE=/path/to/perf.log
#   NODE_BIN=node
#   BUN_BIN=bun
#   QJS_BIN=/path/to/qjs

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
LOG_FILE=${LOG_FILE:-"$SCRIPT_DIR/perf.log"}
NODE_BIN=${NODE_BIN:-node}
BUN_BIN=${BUN_BIN:-bun}
QJS_BIN=${QJS_BIN:-"$SCRIPT_DIR/../target/release/qjs"}

extract_time_ms() {
  awk '/^TIME_MS[[:space:]]+/ { print $2; found=1; exit } END { if (!found) print "NO_TIME"; }'
}

run_with_bin() {
  label=$1
  bin=$2
  script=$3

  if [ "$label" = "QJS" ]; then
    if [ ! -x "$bin" ]; then
      printf 'MISSING'
      return
    fi
  else
    if ! command -v "$bin" >/dev/null 2>&1; then
      printf 'MISSING'
      return
    fi
  fi

  output=$($bin "$script" 2>&1)
  status=$?
  if [ "$status" -ne 0 ]; then
    printf 'ERR(%s)' "$status"
    return
  fi

  time_ms=$(printf '%s\n' "$output" | extract_time_ms)
  printf '%sms' "$time_ms"
}

mkdir -p "$(dirname -- "$LOG_FILE")"

for script in "$SCRIPT_DIR"/*.js; do
  [ -f "$script" ] || continue

  test_name=$(basename -- "$script")
  node_time=$(run_with_bin NODE "$NODE_BIN" "$script")
  bun_time=$(run_with_bin BUN "$BUN_BIN" "$script")
  qjs_time=$(run_with_bin QJS "$QJS_BIN" "$script")

  timestamp=$(date '+%Y-%m-%d %H:%M:%S %z')
  {
    printf '%s\n' "$timestamp"
    printf '%s > NODE %s BUN %s QJS %s\n' "$test_name" "$node_time" "$bun_time" "$qjs_time"
  } >> "$LOG_FILE"

  printf '%s\n' "$timestamp"
  printf '%s > NODE %s BUN %s QJS %s\n' "$test_name" "$node_time" "$bun_time" "$qjs_time"
done

printf 'Log: %s\n' "$LOG_FILE"
