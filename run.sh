#!/bin/sh
# Run the development build staged by ./build.sh.
#
#   ./run.sh [linexin-center options...]
#       Only the staged widgets (build/usr/share/linexin/widgets).
#   ./run.sh --with-system-widgets [linexin-center options...]
#       Also the installed widgets from /usr/share/linexin/widgets. The staged
#       z-system-information-widget.so replaces an installed .py of that name.
#   ./run.sh --install-desktop-entry [DATA_DIR]
#   ./run.sh --remove-desktop-entry [DATA_DIR]
#       Install (or remove) a desktop entry and icon for the staged build in the
#       XDG data directory DATA_DIR (default: build/xdg-data), then exit. Desktop
#       shells only read their own data directories, so pass e.g. ~/.local/share
#       explicitly if you want the panel to show the icon. Nothing is installed
#       anywhere unless you ask for it.
#
# Dev runs start a separate instance (LINEXIN_NEW_INSTANCE=1) so an already
# running Linexin Center is not activated instead; export
# LINEXIN_NEW_INSTANCE= to test the single-instance behavior.
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
stage="$project_dir/build/usr"
binary="$stage/bin/linexin-center"
desktop_file=github.petexy.linexincenter.desktop

if [ ! -x "$binary" ]; then
    echo 'Build and stage the Center first with ./build.sh' >&2
    exit 1
fi

case "${1:-}" in
--install-desktop-entry | --remove-desktop-entry)
    data_dir="${2:-$project_dir/build/xdg-data}"
    entry="$data_dir/applications/$desktop_file"
    icon="$data_dir/icons/hicolor/scalable/apps/github.petexy.linexincenter.svg"
    if [ "$1" = --remove-desktop-entry ]; then
        rm -f -- "$entry" "$icon"
        echo "Removed $entry and $icon"
        exit 0
    fi
    install -Dm644 "$stage/share/icons/github.petexy.linexincenter.svg" "$icon"
    mkdir -p "$data_dir/applications"
    sed -e "s|^Exec=.*|Exec=env LINEXIN_NEW_INSTANCE=1 $binary|" \
        -e "s|^Icon=.*|Icon=$icon|" \
        "$stage/share/applications/$desktop_file" >"$entry"
    echo "Installed $entry"
    exit 0
    ;;
--with-system-widgets)
    shift
    set -- --widgets-dir "$stage/share/linexin/widgets" --widgets-dir /usr/share/linexin/widgets "$@"
    ;;
esac

export LINEXIN_NEW_INSTANCE="${LINEXIN_NEW_INSTANCE-1}"
# Lets GTK find the staged application icon (window controls) without an install.
export XDG_DATA_DIRS="$stage/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "$binary" "$@"
