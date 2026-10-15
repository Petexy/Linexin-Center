#!/bin/sh
# Runs the bridge_check example inside a private, headless KWin (own D-Bus
# session, own Wayland socket) with a fake `sudo` first in PATH, and HOME/XDG
# directories redirected below the output directory. Nothing touches the
# desktop session, the user's configuration or real credentials.
#
# Usage: crates/python-bridge/examples/run_bridge_check.sh [output-dir]
#   LINEXIN_LANG=pl_PL  check the Polish dictionaries (default: en_US)
#   LINEXIN_TEST_SOCKET Wayland socket name (default: linexin-bridge-check)
#   LINEXIN_REFERENCE_WIDGET=/path/widget.py  also render that widget under
#                       plain PyGObject (tests/python/render_reference.py)
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/../../.." && pwd)
out=$(mkdir -p "${1:-$root/target/bridge-check}" && CDPATH='' cd -- "${1:-$root/target/bridge-check}" && pwd)
socket=${LINEXIN_TEST_SOCKET:-linexin-bridge-check}
mkdir -p "$out/bin" "$out/home" "$out/config" "$out/data" "$out/cache" "$out/tmp"

cat > "$out/bin/sudo" <<'EOF'
#!/bin/sh
# Fake sudo: never elevates; accepts only the test password.
case "$1" in
    -k) exit 0 ;;
    -S) IFS= read -r password || exit 1
        [ "$password" = "correct horse" ] && exit 0
        exit 1 ;;
    -A) shift
        password=$("$SUDO_ASKPASS") || exit 1
        [ "$password" = "correct horse" ] || exit 1
        exec "$@" ;;
esac
exit 2
EOF
chmod 755 "$out/bin/sudo"

(cd "$root" && cargo build -p linexin-python-bridge --example bridge_check)
target=$(cd "$root" && cargo metadata --no-deps --format-version 1 |
    python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')

export LINEXIN_FAKE_SUDO_DIR="$out/bin" LINEXIN_BRIDGE_CHECK_OUTPUT="$out"
export LINEXIN_TEST_SOCKET="$socket" LINEXIN_CHECK_BINARY="$target/debug/examples/bridge_check"
export LINEXIN_REFERENCE_SCRIPT="$root/tests/python/render_reference.py"
export PATH="$out/bin:$PATH" TMPDIR="$out/tmp" HOME="$out/home"
export XDG_CONFIG_HOME="$out/config" XDG_DATA_HOME="$out/data" XDG_CACHE_HOME="$out/cache"
# No accessibility bus and no portals: nothing gets activated on the private bus.
export GTK_A11Y=none NO_AT_BRIDGE=1 GDK_DEBUG=no-portals ADW_DISABLE_PORTAL=1
unset DISPLAY WAYLAND_DISPLAY
exec dbus-run-session -- sh -c '
    kwin_wayland --virtual --no-lockscreen --width 1600 --height 1000 \
        --socket "$LINEXIN_TEST_SOCKET" >"$LINEXIN_BRIDGE_CHECK_OUTPUT/kwin.log" 2>&1 &
    kwin=$!
    trap "kill $kwin 2>/dev/null; wait $kwin 2>/dev/null" EXIT
    for _ in $(seq 100); do
        [ -S "$XDG_RUNTIME_DIR/$LINEXIN_TEST_SOCKET" ] && break
        sleep 0.1
    done
    export WAYLAND_DISPLAY="$LINEXIN_TEST_SOCKET"
    if [ -n "${LINEXIN_REFERENCE_WIDGET:-}" ]; then
        python3 -B "$LINEXIN_REFERENCE_SCRIPT" "$LINEXIN_REFERENCE_WIDGET" \
            "$LINEXIN_BRIDGE_CHECK_OUTPUT" "${LINEXIN_LANG:-en_US}"
    fi
    "$LINEXIN_CHECK_BINARY"
'
