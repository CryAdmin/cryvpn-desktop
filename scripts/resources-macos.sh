#!/bin/bash
# Ядро Xray и базы geoip для сборки под macOS (Apple Silicon).
# В репозиторий они не входят. Версия и контрольная сумма зафиксированы;
# при несовпадении суммы сборка останавливается.
set -euo pipefail
cd "$(dirname "$0")/.."
URL="https://github.com/XTLS/Xray-core/releases/download/v26.11.9/Xray-macos-arm64-v8a.zip"
SHA="233f0c66681cd0726f81a6b2b6ed6a5710869e312047b522e46ed53035bebd26"
DST="src-tauri/resources-macos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
mkdir -p "$DST"
curl -fsSL -o "$TMP/x.zip" "$URL"
GOT="$(shasum -a 256 "$TMP/x.zip" | cut -d' ' -f1)"
[ "$GOT" = "$SHA" ] || { echo "Checksum mismatch: expected $SHA, got $GOT"; exit 1; }
unzip -q -o "$TMP/x.zip" -d "$TMP/x"
cp "$TMP/x/xray" "$TMP/x/geoip.dat" "$TMP/x/geosite.dat" "$DST/"
chmod +x "$DST/xray"
"$DST/xray" version | head -1
