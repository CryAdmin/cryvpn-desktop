#!/usr/bin/env bash
# Ядро Xray для iPhone: LibXray.xcframework.
#
# На iPhone ядро не может быть отдельной программой, как на Android или
# компьютере: iOS не даёт запускать процессы. Поэтому ядро — библиотека,
# которую загружает расширение VPN (Network Extension). Собираем её из
# libXray — обёртки авторов Xray над тем же Xray-core v26.11.9, что и на
# остальных системах. Флаги — из их рецепта build/app/apple_go.py; сам
# скрипт не запускаем, только компилируем код.
#
# Исходники сверяем по коммиту: подменённое ядро в приложение попасть
# не должно. Нужны Xcode и Go.
set -euo pipefail

LIBXRAY_COMMIT='5a0f7d8d1c28b34afb5727b04b26a10f6dbac7e3'
MIN_IOS='15.0'

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/src-tauri/ios-libs"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

git -c advice.detachedHead=false clone -q https://github.com/XTLS/libXray.git "$tmp/src"
git -C "$tmp/src" checkout -q "$LIBXRAY_COMMIT"
got="$(git -C "$tmp/src" rev-parse HEAD)"
if [ "$got" != "$LIBXRAY_COMMIT" ]; then
  echo "Исходники libXray не те: ждали $LIBXRAY_COMMIT, получили $got" >&2
  exit 1
fi

# Одна сборка: платформа (iphoneos или iphonesimulator) и архитектура.
build() {
  local sdk="$1" dir="$tmp/lib/$1-arm64"
  local sysroot flags
  sysroot="$(xcrun --sdk "$sdk" --show-sdk-path)"
  flags="-isysroot $sysroot -m$sdk-version-min=$MIN_IOS -arch arm64"
  mkdir -p "$dir"
  (
    cd "$tmp/src"
    GOOS=ios GOARCH=arm64 GOFLAGS=-tags=ios CGO_ENABLED=1 \
    CC="xcrun --sdk $sdk --toolchain $sdk clang" \
    CXX="xcrun --sdk $sdk --toolchain $sdk clang++" \
    CGO_CFLAGS="$flags" CGO_CXXFLAGS="$flags" \
    CGO_LDFLAGS="$flags -Wl,-Bsymbolic-functions" \
    GOTOOLCHAIN="${GOTOOLCHAIN:-auto}" \
    go build -trimpath -ldflags='-s -w -buildid=' -buildmode=c-archive \
      -o "$dir/libXray.a" ./cgo_bridge
  )
}

build iphoneos
build iphonesimulator

mkdir -p "$tmp/include"
cp "$tmp/lib/iphoneos-arm64/libXray.h" "$tmp/include/"
cp "$tmp/src/build/template/module.modulemap" "$tmp/include/"

rm -rf "$out/LibXray.xcframework"
mkdir -p "$out"
xcodebuild -create-xcframework \
  -library "$tmp/lib/iphoneos-arm64/libXray.a" -headers "$tmp/include" \
  -library "$tmp/lib/iphonesimulator-arm64/libXray.a" -headers "$tmp/include" \
  -output "$out/LibXray.xcframework"
ls -la "$out/LibXray.xcframework"

# Базы geoip/geosite для правил маршрутизации — из выпуска Xray той же
# версии, что и на Android, со сверкой контрольной суммы.
GEO_URL='https://github.com/XTLS/Xray-core/releases/download/v26.11.9/Xray-android-arm64-v8a.zip'
GEO_SHA='edcefc25b9b3195b11e18624c8269a969d6aabc39c6298a9a5ab29de15d46f3b'
curl -fsSL --max-time 300 -o "$tmp/xray.zip" "$GEO_URL"
got="$(shasum -a 256 "$tmp/xray.zip" | cut -d' ' -f1)"
if [ "$got" != "$GEO_SHA" ]; then
  echo "Контрольная сумма не сошлась: ждали $GEO_SHA, получили $got" >&2
  exit 1
fi
unzip -q -o "$tmp/xray.zip" geoip.dat geosite.dat -d "$tmp/geo"
mkdir -p "$out/geo"
cp "$tmp/geo/geoip.dat" "$tmp/geo/geosite.dat" "$out/geo/"
ls -la "$out/geo"
