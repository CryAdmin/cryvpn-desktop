## Что нового в 0.1.4

- Вход больше не срывается, когда адрес кабинета не находится через DNS
  (например, при включённом другом VPN): приложение само пробует
  запасной адрес.
- Вход одним нажатием по ссылке `cryvpn://login/…` со страницы подписки.
- На Android у приложения свой ярлык.
- Число устройств в шапке сразу учитывает это устройство.
- Вход через Telegram и приложение для Android — с версии 0.1.3.

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
