#!/bin/bash
# Renders the native widget (harness.rs) and the original Python widget
# (python_reference.py) with identical options in a private headless KWin and
# prints the number of differing pixels of every pair.
#
#   cargo build -p linexin-system-information --lib --examples
#   plugins/system-information/examples/compare.sh [OUTPUT_DIR]
#
# Both renderers run in unprivileged user/mount namespaces where /proc/meminfo
# is a snapshot and /proc/stat's btime makes the uptime read 2d 3h 4m, so live
# values cannot differ. Besides en_US/pl_PL/de_DE x light/dark x wide/narrow it
# covers the error view, uncaught exceptions during loading and the logo lookup.
set -u
examples=$(cd "$(dirname "$0")" && pwd)
workspace=$(cd "$examples/../../.." && pwd)
out=$(realpath -m "${1:-$workspace/target/system-information-compare}")
socket=${LINEXIN_TEST_SOCKET:-linexin-test-sys2}

if [ -z "${LINEXIN_COMPARE_SESSION:-}" ]; then
    mkdir -p "$out/png" "$out/fixtures/icons" "$out/fixtures/bin-noexec" "$out/fixtures/bin-badgpu" \
        "$out/home/config" "$out/home/cache" "$out/home/data"
    cat >"$out/fixtures/bus.conf" <<'EOF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <policy context="default"><allow send_destination="*"/><allow own="*"/></policy>
</busconfig>
EOF
    export LINEXIN_COMPARE_SESSION=1 XDG_CONFIG_HOME=$out/home/config \
        XDG_CACHE_HOME=$out/home/cache XDG_DATA_HOME=$out/home/data \
        ADW_DISABLE_PORTAL=1 GDK_DEBUG=no-portals GTK_A11Y=none
    unset DISPLAY WAYLAND_DISPLAY
    exec dbus-run-session --config-file="$out/fixtures/bus.conf" -- bash -c '
        kwin_wayland --virtual --no-lockscreen --width 1600 --height 1000 --socket "$1" \
            >"$XDG_CACHE_HOME/kwin.log" 2>&1 &
        kwin=$!
        for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/$1" ] && break; sleep 0.1; done
        WAYLAND_DISPLAY=$1 GDK_BACKEND=wayland "$2" "$3"
        status=$?
        kill $kwin; wait $kwin 2>/dev/null
        exit $status' session "$socket" "$0" "$out"
fi

fixtures=$out/fixtures
harness=$workspace/target/debug/examples/harness
rm -f "$out"/png/*.png
rm -f "$fixtures/meminfo" && cat /proc/meminfo >"$fixtures/meminfo"
printf 'NAME="Linexin\n' >"$fixtures/os-release-broken"
for logo in logo mark; do
    { grep -v '^LOGO=' /etc/os-release; echo "LOGO=\"linexin-test-$logo\""; } >"$fixtures/os-release-$logo"
done
svg() { # name, colour: a 192 px disc so each lookup candidate is recognisable
    printf '<svg xmlns="http://www.w3.org/2000/svg" width="192" height="192"><circle cx="96" cy="96" r="90" fill="%s"/></svg>\n' \
        "$2" >"$XDG_DATA_HOME/icons/$1.svg"
}
mkdir -p "$XDG_DATA_HOME/icons"
svg linexin-test-logo-text '#26a269'
svg linexin-test-logo-text-dark '#1c71d8'
svg linexin-test-logo-dark '#813d9c'
svg linexin-test-logo '#c01c28'
svg linexin-test-mark '#e66100'
svg linexin-test-mark-dark '#1a5fb4'
printf '#!/bin/sh\n' >"$fixtures/bin-noexec/ps" && chmod 644 "$fixtures/bin-noexec/ps"
ln -sf "$(command -v ps)" "$fixtures/bin-badgpu/ps"
printf '#!/bin/sh\nprintf "01:00.0 VGA compatible controller: Caf\\351 GPU\\n"\n' >"$fixtures/bin-badgpu/lspci"
chmod 755 "$fixtures/bin-badgpu/lspci"

# pair TAG OPTIONS...: both renderers, top, scrolled to the end and after a
# light/dark switch. OS_RELEASE and WIDGET_PATH optionally replace
# /etc/os-release and PATH for the widgets.
pair() {
    local tag=$1
    shift
    sed "s/^btime .*/btime $(($(date +%s) - 183841))/" /proc/stat >"$fixtures/stat"
    FIXTURES=$fixtures OUTER_UID=$(id -u) OUTER_GID=$(id -g) \
        unshare --user --map-root-user --mount bash -c '
        mount --bind "$FIXTURES/stat" /proc/stat && mount --bind "$FIXTURES/meminfo" /proc/meminfo &&
            { [ -z "${OS_RELEASE:-}" ] || mount --bind "$OS_RELEASE" /etc/os-release; } || exit 1
        exec unshare --user --map-user="$OUTER_UID" --map-group="$OUTER_GID" bash -c '\''
            tag=$1 harness=$2 reference=$3 png=$4
            shift 4
            [ -n "${WIDGET_PATH:-}" ] && export PATH=$WIDGET_PATH
            for impl in rust py; do
                args=("$@" --output "$png/$impl-$tag.png" --bottom-output "$png/$impl-$tag-bottom.png"
                    --switch-output "$png/$impl-$tag-switched.png")
                if [ $impl = rust ]; then "$harness" "${args[@]}"; else /usr/bin/python3 "$reference" "${args[@]}"; fi
            done'\'' pair "$@"' frozen "$tag" "$harness" "$examples/python_reference.py" "$out/png" "$@" \
        >>"$out/log.txt" 2>&1
}

: >"$out/log.txt"
for lang in en_US pl_PL de_DE; do
    for scheme in light dark; do
        flag=$([ $scheme = dark ] && echo --dark)
        pair "$lang-wide-$scheme" --lang $lang $flag --width 900 --height 1000
        pair "$lang-narrow-$scheme" --lang $lang $flag --width 420 --height 760
    done
done
OS_RELEASE=$fixtures/os-release-broken pair error --lang pl_PL --width 700 --height 700
WIDGET_PATH=$fixtures/bin-noexec pair partial-ps --lang de_DE --width 900 --height 1000
WIDGET_PATH=$fixtures/bin-badgpu pair partial-lspci --lang en_US --width 900 --height 1000
for logo in logo mark; do
    OS_RELEASE=$fixtures/os-release-$logo pair "logo-$logo-light" --width 420 --height 760
    OS_RELEASE=$fixtures/os-release-$logo pair "logo-$logo-dark" --dark --width 420 --height 760
done
"$harness" --hide-sidebar --width 800 --height 600 2>/dev/null | grep -v '^$'
"$harness" --destroy-early 2>/dev/null | grep -v '^$'

python3 - "$out/png" <<'EOF'
import pathlib, sys
from PIL import Image, ImageChops
differing = 0
pairs = sorted(pathlib.Path(sys.argv[1]).glob("py-*.png"))
for py in pairs:
    native = py.with_name("rust-" + py.name[3:])
    a, b = Image.open(py).convert("RGBA"), Image.open(native).convert("RGBA")
    box = ImageChops.difference(a, b).getbbox(alpha_only=False) if a.size == b.size else "size"
    differing += box is not None
    print(f"{py.name[3:]}: {'identical' if box is None else f'differs {box}'}")
print(f"{len(pairs)} pairs, {differing} differing")
sys.exit(1 if differing or not pairs else 0)
EOF
