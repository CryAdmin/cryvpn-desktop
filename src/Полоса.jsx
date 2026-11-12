import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

// Своя верхняя полоса окна вместо системной: та в Windows белая и режет
// окно пополам. Эта прозрачная — фон окна идёт под ней без шва. За неё
// окно таскают, в ней же кнопки «свернуть», «закрыть» и весть об обновлении.

const ЧАС = 60 * 60 * 1000;

export default function Полоса() {
  const [версия, установитьВерсию] = useState(null);
  const [шаг, установитьШаг] = useState("ждём"); // ждём | качаем | беда
  const окно = getCurrentWindow();

  useEffect(() => {
    const спросить = () =>
      invoke("обновление")
        .then((новая) => установитьВерсию(новая))
        .catch(() => {});
    спросить();
    const часы = setInterval(спросить, ЧАС);
    return () => clearInterval(часы);
  }, []);

  const обновиться = async () => {
    if (шаг === "качаем") return;
    установитьШаг("качаем");
    try {
      // При удаче приложение закроется само и откроется уже новым.
      await invoke("обновиться");
    } catch {
      установитьШаг("беда");
      setTimeout(() => установитьШаг("ждём"), 4000);
    }
  };

  const подпись =
    шаг === "качаем" ? "Обновляю…" : шаг === "беда" ? "Не вышло, ещё раз" : "Обновить";

  return (
    <div className="полоса-окна" data-tauri-drag-region>
      {версия && (
        <button
          className={`обнова ${шаг}`}
          onClick={обновиться}
          title={`Вышла версия ${версия}. Нажмите — приложение обновится и откроется снова`}
        >
          <span className="обнова-точка" />
          {подпись}
        </button>
      )}
      <button className="окно-кнопка" onClick={() => окно.minimize()} aria-label="Свернуть">
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
          <path d="M0 5h10" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
      <button
        className="окно-кнопка закрыть"
        onClick={() => окно.close()}
        aria-label="Закрыть"
      >
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
          <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
    </div>
  );
}
