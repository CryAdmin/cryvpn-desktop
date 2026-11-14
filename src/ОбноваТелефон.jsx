import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// Android: о новой версии сообщает плашка сверху. По нажатию приложение
// само скачивает APK из выпусков открытого репозитория, сверяет его
// контрольную сумму и отдаёт системе — та показывает окно установки
// поверх. Поставить за человека нельзя: согласие спрашивает сам Android.
const АДРЕС_APK =
  "https://github.com/CryAdmin/cryvpn-desktop/releases/latest/download/CryVPN-Android.apk";
const ЧАС = 60 * 60 * 1000;

export default function ОбноваТелефон() {
  const [версия, установитьВерсию] = useState(null);
  const [шаг, установитьШаг] = useState("ждём"); // ждём | качаем | беда

  useEffect(() => {
    // Проверяем при запуске, раз в час и когда человек возвращается в окно,
    // но не чаще раза в пять минут. Неудачная проверка плашку не снимает:
    // раз узнали о новой версии — помним.
    let спрошено = 0;
    const спросить = () => {
      спрошено = Date.now();
      invoke("обновление")
        .then((новая) => установитьВерсию((была) => новая ?? была))
        .catch(() => {});
    };
    const вернулись = () => {
      if (!document.hidden && Date.now() - спрошено > 5 * 60 * 1000) спросить();
    };
    спросить();
    const часы = setInterval(спросить, ЧАС);
    window.addEventListener("focus", вернулись);
    document.addEventListener("visibilitychange", вернулись);
    return () => {
      clearInterval(часы);
      window.removeEventListener("focus", вернулись);
      document.removeEventListener("visibilitychange", вернулись);
    };
  }, []);

  // Плашка занимает место сверху — экран под ней должен стать ниже.
  useEffect(() => {
    document.documentElement.classList.toggle("с-обновой", !!версия);
  }, [версия]);

  const обновиться = async () => {
    if (шаг === "качаем") return;
    // Не вышло скачать самим — второй раз отдаём ссылку браузеру: пусть
    // человек останется хотя бы с этим путём.
    if (шаг === "беда") {
      invoke("открыть_ссылку", { адрес: АДРЕС_APK }).catch(() => {});
      установитьШаг("ждём");
      return;
    }
    установитьШаг("качаем");
    try {
      await invoke("обновиться");
      установитьШаг("ждём");
    } catch {
      установитьШаг("беда");
    }
  };

  if (!версия) return null;
  return (
    <button className={`обнова-телефон ${шаг}`} onClick={обновиться}>
      <span className="обнова-точка" />
      {шаг === "качаем"
        ? "Скачиваю обновление…"
        : шаг === "беда"
          ? "Не вышло — нажмите, чтобы скачать через браузер"
          : `Вышла версия ${версия} — обновить`}
    </button>
  );
}
