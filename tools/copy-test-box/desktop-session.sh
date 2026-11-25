#!/bin/sh
# The desktop session of the copy test's box (decision 0459). The runner,
# tools/copy_test.py, mounts this file read-only in the box and runs it there as
# the box's user, never as root. It names no application: the recipe gives the
# program and its arguments.
#
#   desktop-session.sh start <program> [argument ...]
#       A virtual display, a session bus, and the accessibility bus, each the
#       box's own, then the program, which takes this shell's place. Started by
#       the runner with `docker exec -d`.
#   desktop-session.sh ready <program>
#       Exits 0 when the accessibility registry lists an application whose
#       process runs <program>, and 1 when it does not yet.
#   desktop-session.sh list
#       One line for each application the registry lists:
#       A2M_APPLICATION <bus name> <program> <name it gives itself>
#
# Nothing here reaches outside the box: the display listens on no TCP port, and
# both buses are sockets in a folder of the box's own.
set -eu

RUN=/tmp/a2m-desktop
BUS="unix:path=$RUN/bus"
SCREEN=:99

say() { echo "a2m-desktop: $*" >&2; }

need() {
    for program in "$@"; do
        if [ ! -x "$program" ] && ! command -v "$program" >/dev/null 2>&1; then
            say "the box's image holds no $program, which a desktop session needs"
            exit 4
        fi
    done
}

# The accessibility bus's address, asked of the session bus.
a11y() {
    out=$(dbus-send --bus="$BUS" --print-reply=literal --dest=org.a11y.Bus \
        /org/a11y/bus org.a11y.Bus.GetAddress 2>/dev/null) || return 1
    out=$(printf '%s' "$out" | tr -d ' \n')
    [ -n "$out" ] || return 1
    printf '%s\n' "$out"
}

# Each application the registry lists, as `<bus name> <program> <name>`.
applications() {
    address=$(a11y) || return 1
    dbus-send --bus="$address" --print-reply --dest=org.a11y.atspi.Registry \
        /org/a11y/atspi/accessible/root org.a11y.atspi.Accessible.GetChildren 2>/dev/null |
        sed -n 's/^ *string "\(:[0-9.]*\)"$/\1/p' |
        while read -r name; do
            process=$(dbus-send --bus="$address" --print-reply=literal --dest=org.freedesktop.DBus \
                /org/freedesktop/DBus org.freedesktop.DBus.GetConnectionUnixProcessID \
                string:"$name" 2>/dev/null | awk '{print $NF}') || continue
            program=$(readlink -f "/proc/$process/exe" 2>/dev/null) || continue
            own=$(dbus-send --bus="$address" --print-reply=literal --dest="$name" \
                /org/a11y/atspi/accessible/root org.freedesktop.DBus.Properties.Get \
                string:org.a11y.atspi.Accessible string:Name 2>/dev/null |
                sed 's/^ *variant *//') || own=""
            echo "$name $program $own"
        done
}

# Wait, at most `tries` tenths of a second, until `condition` holds.
until_holds() {
    tries=$1
    shift
    while ! "$@" >/dev/null 2>&1; do
        tries=$((tries - 1))
        if [ "$tries" -le 0 ]; then
            return 1
        fi
        sleep 0.1
    done
}

case "${1:-}" in
start)
    shift
    [ "$#" -ge 1 ] || { say "start needs a program"; exit 2; }
    need Xvfb dbus-daemon dbus-send /usr/libexec/at-spi-bus-launcher "$1"
    [ "$(id -u)" != 0 ] || { say "a desktop session is a person's and never runs as root"; exit 2; }
    mkdir -p -m 0700 "$RUN"
    export XDG_RUNTIME_DIR="$RUN" DISPLAY="$SCREEN" DBUS_SESSION_BUS_ADDRESS="$BUS"
    Xvfb "$SCREEN" -nolisten tcp -screen 0 1280x800x24 >"$RUN/display.log" 2>&1 &
    until_holds 300 test -S "/tmp/.X11-unix/X${SCREEN#:}" || { say "the display did not start"; exit 3; }
    dbus-daemon --session --address="$BUS" --nofork --nopidfile >"$RUN/session-bus.log" 2>&1 &
    until_holds 300 test -S "$RUN/bus" || { say "the session bus did not start"; exit 3; }
    /usr/libexec/at-spi-bus-launcher --launch-immediately >"$RUN/accessibility-bus.log" 2>&1 &
    until_holds 300 a11y || { say "the accessibility bus did not start"; exit 3; }
    exec "$@" >"$RUN/application.log" 2>&1
    ;;
ready)
    [ "$#" -eq 2 ] || { say "ready needs a program"; exit 2; }
    want=$(readlink -f "$2") || exit 1
    applications | awk -v want="$want" '$2 == want {found = 1} END {exit found ? 0 : 1}'
    ;;
list)
    applications | sed 's/^/A2M_APPLICATION /'
    ;;
*)
    say "usage: desktop-session.sh start <program> [argument ...] | ready <program> | list"
    exit 2
    ;;
esac
