#!/usr/bin/env bash
# Ядро Xray и базы geoip/geosite для сборки под Android (arm64).
#
# В репозиторий они не входят: это чужие бинарники. Версия и контрольная
# сумма зафиксированы здесь. Скачанное сверяется с суммой, и при
# несовпадении сборка останавливается: подменённое ядро в APK попасть
# не должно.
set -euo pipefail

URL='https://github.com/XTLS/Xray-core/releases/download/v26.11.9/Xray-android-arm64-v8a.zip'
SHA='edcefc25b9b3195b11e18624c8269a969d6aabc39c6298a9a5ab29de15d46f3b'

root="$(cd "$(dirname "$0")/.." && pwd)"
main="$root/src-tauri/gen/android/app/src/main"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL --max-time 300 -o "$tmp/xray.zip" "$URL"
if command -v sha256sum >/dev/null; then got="$(sha256sum "$tmp/xray.zip" | cut -d' ' -f1)"; else got="$(shasum -a 256 "$tmp/xray.zip" | cut -d' ' -f1)"; fi
if [ "$got" != "$SHA" ]; then
  echo "Контрольная сумма не сошлась: ждали $SHA, получили $got" >&2
  exit 1
fi
unzip -q -o "$tmp/xray.zip" -d "$tmp/xray"

# Android запускает программы только из папки библиотек, поэтому ядро
# лежит в APK под именем библиотеки.
mkdir -p "$main/jniLibs/arm64-v8a" "$main/assets"
cp "$tmp/xray/xray" "$main/jniLibs/arm64-v8a/libxray.so"
cp "$tmp/xray/geoip.dat" "$tmp/xray/geosite.dat" "$main/assets/"
ls -la "$main/jniLibs/arm64-v8a" "$main/assets"
