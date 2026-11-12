import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// Android: сама себя программа там не обновляет — ставить приложения может
// только система. Поэтому сообщаем о новой версии и по нажатию отдаём
// свежий APK: телефон скачает его и предложит установить поверх.
// Адрес один и зашит здесь — выпуски открытого репозитория.
const АДРЕС_APK =
  "https://github.com/CryAdmin/cryvpn-desktop/releases/latest/download/CryVPN-Android.apk";
const ЧАС = 60 * 60 * 1000;

export default function ОбноваТелефон() {
  const [версия, установитьВерсию] = useState(null);

  useEffect(() => {
    const спросить = () =>
      invoke("обновление")
        .then((новая) => установитьВерсию(новая))
        .catch(() => {});
    спросить();
    const часы = setInterval(спросить, ЧАС);
    return () => clearInterval(часы);
  }, []);

  // Плашка занимает место сверху — экран под ней должен стать ниже.
  useEffect(() => {
    document.documentElement.classList.toggle("с-обновой", !!версия);
  }, [версия]);

  if (!версия) return null;
  return (
    <button
      className="обнова-телефон"
      onClick={() => invoke("открыть_ссылку", { адрес: АДРЕС_APK }).catch(() => {})}
    >
      <span className="обнова-точка" />
      Вышла версия {версия} — обновить
    </button>
  );
}
