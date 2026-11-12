## Что нового в 0.1.8

- Задержка до серверов меряется по настоящему ответу сервера, а не по
  времени установки соединения: в мобильных сетях старый замер показывал
  несколько миллисекунд до любого сервера.
- Срок подписки показывается один раз — в объявлении.
- С 0.1.7: Android обновляется из самого приложения.

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
