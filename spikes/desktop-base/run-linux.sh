#!/bin/sh
# Throwaway. Runs the Linux spike with no desktop: a virtual X display, a
# private session bus (the accessibility bus is started from it on demand),
# the test window, then the spike. Everything it starts it stops.
# Needs: Xvfb, dbus-daemon, at-spi2-core, python3-gi with GTK 3.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
Xvfb :87 -nolisten tcp >"$work/xvfb.log" 2>&1 &
xvfb=$!
trap 'kill $xvfb 2>/dev/null; rm -rf "$work"' EXIT
export DISPLAY=:87
dbus-run-session -- sh -c "
  /usr/bin/python3.12 '$here/stand/window.py' '$work/store.json' >'$work/stand.log' 2>&1 &
  stand=\$!
  # the window prints 'ready' once it is shown; wait on that line, not a time
  until grep -q ready '$work/stand.log' 2>/dev/null; do
    kill -0 \$stand || { cat '$work/stand.log'; exit 1; }
    sleep 0.1
  done
  '$here/target/debug/spike-desktop-linux-atspi' '$work/store.json'
  status=\$?
  kill \$stand
  exit \$status
"
