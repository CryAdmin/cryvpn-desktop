#!/bin/bash
# Сборка CryVPN под macOS: приложение, подпись, установочный .dmg.
#
# Зачем отдельный скрипт, а не просто `tauri build`. Сборщик подписывает
# только сам исполняемый файл, а пакет целиком — нет. На маке, где
# приложение собрано, это незаметно. У человека, скачавшего файл, macOS
# проверяет подпись строго и отвечает «приложение не может быть открыто» —
# без объяснений и без кнопки «всё равно открыть». Так первая сборка не
# запустилась у первого же получателя.
#
# Подпись здесь без сертификата (ad-hoc): она не делает приложение
# доверенным, но делает его целым. Настоящая подпись и нотаризация Apple
# встанут на это же место, когда появится аккаунт разработчика.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:$HOME/.cargo/bin:$PATH"

[ -x src-tauri/resources-macos/xray ] || scripts/resources-macos.sh

VERSION=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
APP="src-tauri/target/release/bundle/macos/CryVPN.app"
OUT="src-tauri/target/release/bundle/dmg"
DMG="$OUT/CryVPN_${VERSION}_aarch64.dmg"

npm run tauri build 2>&1 | grep -E "^error|Error |Finished" || true
[ -d "$APP" ] || { echo "приложение не собралось"; exit 1; }

codesign --force --deep --sign - "$APP"
# Без этой проверки выпускать нельзя: именно её не прошла первая сборка.
codesign --verify --deep --strict "$APP"
echo "подпись пакета цела"

STAGE="$(mktemp -d)"; trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
mkdir -p "$OUT"; rm -f "$DMG"
hdiutil create -volname "CryVPN" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
echo "готово: $DMG ($(stat -f%z "$DMG") байт)"
echo "sha256: $(shasum -a 256 "$DMG" | cut -d' ' -f1)"
