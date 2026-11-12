## Что нового в 0.1.7

- Android: обновление из самого приложения. По нажатию на плашку оно
  скачивает APK из этого репозитория, сверяет SHA-256 и отдаёт системе —
  та спрашивает согласие и ставит поверх. В первый раз Android попросит
  разрешить установку из CryVPN.
- Android теперь представляется серверу как Android, а не как Windows.
- Имя устройства с русскими буквами больше не превращается в кракозябры
  в списке устройств.
- С 0.1.6: программы мимо VPN, уведомление о подключении на Android.

## Android

`CryVPN-Android.apk` — для телефонов на arm64. При установке Android
предупредит о неизвестном источнике: приложение ставится не из магазина.
Проверить файл: сравнить SHA-256 с `CryVPN-Android.apk.sha256`.

## Установка

Скачайте `CryVPN-Setup.exe` и запустите. Пока выпуски не подписаны,
Windows покажет предупреждение о неизвестном издателе: нажмите
«Подробнее» → «Выполнить в любом случае».

Проверить файл: `certutil -hashfile CryVPN-Setup.exe SHA256` и сравнить
с содержимым `CryVPN-Setup.exe.sha256`.

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io),
certificate by [SignPath Foundation](https://signpath.org).

Заявка находится на рассмотрении. Правила подписи, состав команды и
privacy policy — в разделе
[Code signing policy](https://github.com/CryAdmin/cryvpn-desktop#code-signing-policy).
Подписываются только сборки, полученные автоматически из этого
репозитория.
