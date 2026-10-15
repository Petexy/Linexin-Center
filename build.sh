#!/bin/sh
# Debug build of the workspace, staged as an install tree under build/usr:
#
#   build/usr/bin/linexin-center
#   build/usr/share/linexin/widgets/z-system-information-widget.so
#   build/usr/share/linexin/widgets/localization/<lang>/*_dictionary.py
#   build/usr/share/icons/{github.petexy.linexincenter,computer-linexin}.svg
#   build/usr/share/applications/github.petexy.linexincenter.desktop
#
# build/usr/bin/linexin-center finds ../share/linexin/widgets and uses it instead
# of /usr/share/linexin/widgets (the original "running from source" rule).
# Nothing outside this directory is touched. Run it with ./run.sh.
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$project_dir"

cargo build --workspace
# cargo metadata honors CARGO_TARGET_DIR and Cargo configuration.
target_dir=$(cargo metadata --no-deps --format-version 1 |
    python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')

stage="$project_dir/build/usr"
rm -rf -- "${stage:?}"
widgets="$stage/share/linexin/widgets"
install -Dm755 "$target_dir/debug/linexin-center" "$stage/bin/linexin-center"
install -Dm644 "$target_dir/debug/liblinexin_system_information.so" \
    "$widgets/z-system-information-widget.so"
for dictionary in data/localization/*/*_dictionary.py; do
    install -Dm644 "$dictionary" "$widgets/localization/${dictionary#data/localization/}"
done
install -Dm644 data/icons/github.petexy.linexincenter.svg \
    "$stage/share/icons/github.petexy.linexincenter.svg"
install -Dm644 data/icons/computer-linexin.svg "$stage/share/icons/computer-linexin.svg"
install -Dm644 packaging/github.petexy.linexincenter.desktop \
    "$stage/share/applications/github.petexy.linexincenter.desktop"
echo "Staged $stage; run ./run.sh"
