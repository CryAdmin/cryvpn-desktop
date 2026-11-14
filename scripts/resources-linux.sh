#!/usr/bin/env bash
# Ядро Xray и базы geoip/geosite для сборки под Linux.
#
# В репозиторий они не входят: это чужие бинарники. Версия и контрольные
# суммы зафиксированы здесь. Скачанное сверяется с суммой, и при
# несовпадении сборка останавливается: подменённое ядро в пакет попасть
# не должно. Версия должна совпадать с остальными scripts/resources*.
set -euo pipefail

VERSION='v26.11.9'
case "$(uname -m)" in
  x86_64)  FILE='Xray-linux-64.zip';        SHA='b651f7e1d9966ee2c1b1ee1bf8b52c72771824d3065dce96bbc8bcc19ed95f82' ;;
  aarch64) FILE='Xray-linux-arm64-v8a.zip'; SHA='64ed6858982a7f5eb85d88b811089063bd4f17561f4c8f44e0f1dc8e6f6430be' ;;
  *) echo "Неизвестная архитектура: $(uname -m)" >&2; exit 1 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
dst="$root/src-tauri/resources-linux"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL --max-time 300 -o "$tmp/xray.zip" "https://github.com/XTLS/Xray-core/releases/download/$VERSION/$FILE"
got="$(sha256sum "$tmp/xray.zip" | cut -d' ' -f1)"
if [ "$got" != "$SHA" ]; then
  echo "Контрольная сумма не сошлась: ждали $SHA, получили $got" >&2
  exit 1
fi
unzip -q -o "$tmp/xray.zip" -d "$tmp/xray"

mkdir -p "$dst"
cp "$tmp/xray/xray" "$tmp/xray/geoip.dat" "$tmp/xray/geosite.dat" "$dst/"
chmod +x "$dst/xray"
"$dst/xray" version | head -1
