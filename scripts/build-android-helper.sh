#!/usr/bin/env bash
# Собирает libcryspawn.so — маленькую библиотеку, которой служба VPN на
# Android запускает ядро. Кладёт её рядом с ядром в папку библиотек APK.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
ndk="${NDK_HOME:-${ANDROID_NDK_LATEST_HOME:-${ANDROID_NDK_HOME:-}}}"
if [ -z "$ndk" ] || [ ! -d "$ndk" ]; then
  echo "Не найден Android NDK: задайте NDK_HOME" >&2
  exit 1
fi
bin="$(ls -d "$ndk"/toolchains/llvm/prebuilt/*/bin | head -1)"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android24-clang"
export CC_aarch64_linux_android="$bin/aarch64-linux-android24-clang"
export AR_aarch64_linux_android="$bin/llvm-ar"

cargo build --release --target aarch64-linux-android --manifest-path "$root/src-tauri/cryspawn/Cargo.toml"
out="$root/src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$out"
cp "$root/src-tauri/cryspawn/target/aarch64-linux-android/release/libcryspawn.so" "$out/"
ls -la "$out/libcryspawn.so"
