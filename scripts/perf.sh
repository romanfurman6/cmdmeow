#!/bin/bash
# Measure what Cmd+Meow costs: launch, CPU and memory hidden, with the panel
# open (still and with the mouse moving over it), and with the app window
# open. macOS only; run from Terminal (it needs Accessibility to move the
# mouse and click the menu bar icon).
#
#   scripts/perf.sh <path to the cmdmeow binary> <label> [seconds per phase]
#
# Run the binary inside a bundle (scripts/bundle-mac.sh) whose URL_SCHEME
# is given here as SCHEME (default cmdmeow), so the app window opens by link.
#
# Prints one line per phase and writes perf-<label>.txt, plus the app's own
# repaint log (fps, ms per frame, painted vs unchanged frames) in
# perf-<label>.log and a `sample` profile of the mouse-over-panel phase in
# perf-<label>-sample.txt.
set -u
BIN=${1:?binary}
LABEL=${2:?label}
SECS=${3:-20}
OUT="perf-$LABEL.txt"
LOG="perf-$LABEL.log"
HERE=$(cd "$(dirname "$0")" && pwd)
MOVER="$HERE/perf-mouse.swift"

pkill -x cmdmeow 2>/dev/null
sleep 1
: >"$OUT"

CMDMEOW_REPAINT_LOG=1 "$BIN" >"$LOG" 2>&1 &
PID=$!
START=$(date +%s)
for _ in $(seq 1 60); do
  grep -q "first reading" "$LOG" && break
  sleep 0.5
done
echo "launch: $(grep -m1 'first pass' "$LOG" | sed 's/.*first pass //'), $(grep -m1 'first reading' "$LOG" | sed 's/.*after /first numbers after /')" | tee -a "$OUT"

# cumulative CPU seconds of the app (ps prints [[dd-]hh:]mm:ss.cc)
cpu() {
  ps -o time= -p "$PID" | awk '{n=split($1,a,/[-:]/); s=0; for(i=1;i<=n;i++) s=s*60+a[i]; print s}'
}
rss() { ps -o rss= -p "$PID" | awk '{printf "%.0f", $1/1024}'; }

phase() { # name seconds [command run meanwhile]
  local name=$1 secs=$2
  shift 2
  local c0 c1
  c0=$(cpu)
  if [ $# -gt 0 ]; then "$@"; else sleep "$secs"; fi
  c1=$(cpu)
  echo "$name: cpu $(echo "($c1 - $c0) * 100 / $secs" | bc -l | xargs printf '%.1f')%, rss $(rss) MB" | tee -a "$OUT"
}

# the panel's frame on screen (x y w h), from the window list
panel_rect() {
  swift -e 'import CoreGraphics
let l = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in l where (w["kCGWindowOwnerPID"] as? Int32) == '"$PID"' {
  let b = w["kCGWindowBounds"] as! [String: Double]
  if b["Height"]! > 200 { print(Int(b["X"]!), Int(b["Y"]!), Int(b["Width"]!), Int(b["Height"]!)); break }
}' 2>/dev/null
}

move_over() { # seconds: circle the mouse over the app's front window
  read -r x y w h < <(panel_rect)
  if [ -z "${x:-}" ]; then
    echo "  (no window on screen to move over)"
    sleep "$1"
    return
  fi
  swift "$MOVER" $((x + w / 2)) $((y + h / 2)) $((w / 3)) "$1"
}

click_tray() {
  osascript -e 'tell application "System Events" to tell (first process whose unix id is '"$PID"') to click menu bar item 1 of menu bar 2' >/dev/null 2>&1
}

sleep 10
phase "hidden, idle" "$SECS"

click_tray
sleep 2
phase "panel open, still" "$SECS"
phase "panel open, mouse moving" "$SECS" move_over "$SECS"
(sample "$PID" 8 -file "perf-$LABEL-sample.txt" >/dev/null 2>&1 &)
move_over 9 >/dev/null
click_tray
sleep 2

open "${SCHEME:-cmdmeow}://open/window"
sleep 3
phase "app window, still" "$SECS"
phase "app window, mouse moving" "$SECS" move_over "$SECS"
osascript -e 'tell application "System Events" to tell (first process whose unix id is '"$PID"') to keystroke "w" using command down' >/dev/null 2>&1
sleep 2
phase "hidden again" "$SECS"

kill "$PID" 2>/dev/null
echo "total $(($(date +%s) - START)) s; repaint log in $LOG, profile in perf-$LABEL-sample.txt"
