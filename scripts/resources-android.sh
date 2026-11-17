#!/usr/bin/env bash
# Ядро Xray и базы geoip/geosite для сборки под Android.
#
# В репозиторий они не входят: это чужие бинарники. Версия и контрольная
# сумма зафиксированы здесь. Скачанное сверяется с суммой, и при
# несовпадении сборка останавливается: подменённое ядро в APK попасть
# не должно.
#
# Архитектуры две. arm64 — готовая сборка Xray. 32-битный ARMv7 нужен
# бюджетным Samsung вроде Galaxy A13: процессор у них 64-битный, а Android
# 32-битный, и APK только под arm64 туда не ставится («несовместимо»).
# Готового Android-ядра под ARMv7 Xray не выпускает, поэтому собираем его
# сами — из исходников той же версии, тем же рецептом, что и Xray свои
# Android-сборки (Go + NDK). Исходники сверяем по коммиту тега.
set -euo pipefail

VERSION='v26.11.9'
URL="https://github.com/XTLS/Xray-core/releases/download/$VERSION/Xray-android-arm64-v8a.zip"
SHA='edcefc25b9b3195b11e18624c8269a969d6aabc39c6298a9a5ab29de15d46f3b'
SOURCE_COMMIT='676eaf66f8f05d4d558ee7a74b4736623d3feeb9'

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
mkdir -p "$main/jniLibs/arm64-v8a" "$main/jniLibs/armeabi-v7a" "$main/assets"
cp "$tmp/xray/xray" "$main/jniLibs/arm64-v8a/libxray.so"
cp "$tmp/xray/geoip.dat" "$tmp/xray/geosite.dat" "$main/assets/"

# ── Ядро под ARMv7 ──
ndk="${NDK_HOME:-${ANDROID_NDK_LATEST_HOME:-${ANDROID_NDK_HOME:-}}}"
if [ -z "$ndk" ] || [ ! -d "$ndk" ]; then
  echo "Не найден Android NDK: задайте NDK_HOME" >&2
  exit 1
fi
if ! command -v go >/dev/null; then
  echo "Нужен Go: ядро под 32-битный ARM собирается из исходников" >&2
  exit 1
fi
git -c advice.detachedHead=false clone -q --depth 1 --branch "$VERSION" https://github.com/XTLS/Xray-core.git "$tmp/src"
got="$(git -C "$tmp/src" rev-parse HEAD)"
if [ "$got" != "$SOURCE_COMMIT" ]; then
  echo "Исходники Xray не те: ждали коммит $SOURCE_COMMIT, получили $got" >&2
  exit 1
fi
bin="$(ls -d "$ndk"/toolchains/llvm/prebuilt/*/bin | head -1)"
(
  cd "$tmp/src"
  # Флаги — из выпускной сборки Xray для Android (.github/workflows/release.yml).
  GOOS=android GOARCH=arm GOARM=7 CGO_ENABLED=1 \
  CC="$bin/armv7a-linux-androideabi24-clang" \
  GOTOOLCHAIN="${GOTOOLCHAIN:-auto}" \
  go build -o "$tmp/xray-armv7" -trimpath -buildvcs=false -gcflags="all=-l=4" \
    -ldflags="-X github.com/xtls/xray-core/core.build=${SOURCE_COMMIT:0:7} -s -w -buildid= -checklinkname=0 -extldflags=-Wl,--pack-dyn-relocs=android" \
    ./main
)
cp "$tmp/xray-armv7" "$main/jniLibs/armeabi-v7a/libxray.so"
ls -la "$main/jniLibs/arm64-v8a" "$main/jniLibs/armeabi-v7a" "$main/assets"

# Помощник службы VPN: собирается из src-tauri/cryspawn.
"$(dirname "$0")/build-android-helper.sh"
