#!/bin/sh
# Renders the popup's LiveGraphs.qml outside Plasma with made-up data and
# saves a PNG, to check the graphs without reloading plasmashell.
#   data/plasmoid/tools/preview-graphs.sh [out.png] [dark]
# Needs qt6-declarative's `qml`, kirigami and kquickcharts.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=$(realpath -m "${1:-graphs-preview.png}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Plasma provides i18n()/i18nc(); outside it, stand-ins in the root object.
sed 's|^ColumnLayout {$|ColumnLayout {\
    function i18nc(c, s) { for (let i = 2; i < arguments.length; i++) s = s.replace("%" + (i - 1), arguments[i]); return s; }|' \
    "$here/../org.scdr.widget/contents/ui/LiveGraphs.qml" > "$work/LiveGraphs.qml"
cp "$here/preview-graphs.qml" "$work/main.qml"

[ "${2:-}" = dark ] && export QT_QUICK_CONTROLS_STYLE=org.kde.desktop KDE_COLOR_SCHEME_PATH=/usr/share/color-schemes/BreezeDark.colors
# KQuickCharts draws nothing on the offscreen platform, so this opens a real
# window for a second or two when there's a Wayland session.
platform=offscreen
[ -n "${WAYLAND_DISPLAY:-}" ] && platform=wayland
(cd "$work" && QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=$platform timeout 30 /usr/lib/qt6/bin/qml main.qml)
mv "$work/graphs-preview.png" "$out"
echo "wrote $out"
