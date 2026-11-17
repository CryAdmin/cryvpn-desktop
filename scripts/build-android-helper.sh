#!/usr/bin/env bash
# Собирает libcryspawn.so — маленькую библиотеку, которой служба VPN на
# Android запускает ядро. Кладёт её рядом с ядром в папку библиотек APK,
# для каждой архитектуры свою: arm64 и 32-битный ARMv7 (бюджетные Samsung
# вроде Galaxy A13 работают на 32-битном Android).
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
export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$bin/armv7a-linux-androideabi24-clang"
export CC_armv7_linux_androideabi="$bin/armv7a-linux-androideabi24-clang"
export AR_armv7_linux_androideabi="$bin/llvm-ar"

for pair in aarch64-linux-android:arm64-v8a armv7-linux-androideabi:armeabi-v7a; do
  target="${pair%%:*}"
  abi="${pair##*:}"
  cargo build --release --target "$target" --manifest-path "$root/src-tauri/cryspawn/Cargo.toml"
  out="$root/src-tauri/gen/android/app/src/main/jniLibs/$abi"
  mkdir -p "$out"
  cp "$root/src-tauri/cryspawn/target/$target/release/libcryspawn.so" "$out/"
  ls -la "$out/libcryspawn.so"
done
