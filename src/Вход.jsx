import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// Два шага, как на сайте: почта, потом код из письма.
const ШАГ_ПОЧТА = "почта";
const ШАГ_КОД = "код";

export default function Вход({ вошли }) {
  const [шаг, установитьШаг] = useState(ШАГ_ПОЧТА);
  const [почта, установитьПочту] = useState("");
  const [код, установитьКод] = useState("");
  const [занят, установитьЗанятость] = useState(false);
  const [ошибка, установитьОшибку] = useState("");

  const выслатьКод = async (событие) => {
    событие.preventDefault();
    установитьЗанятость(true);
    установитьОшибку("");
    try {
      const ответ = await invoke("выслать_код", { почта });
      if (ответ.ok) установитьШаг(ШАГ_КОД);
      else установитьОшибку(ответ.сообщение);
    } catch (беда) {
      установитьОшибку(String(беда));
    } finally {
      установитьЗанятость(false);
    }
  };

  const войти = async (событие) => {
    событие.preventDefault();
    установитьЗанятость(true);
    установитьОшибку("");
    try {
      const ответ = await invoke("проверить_код", { почта, код });
      if (!ответ.ok) {
        установитьОшибку(ответ.сообщение);
        return;
      }
      вошли(await invoke("профиль"));
    } catch (беда) {
      установитьОшибку(String(беда));
    } finally {
      установитьЗанятость(false);
    }
  };

  return (
    <main className="экран по-центру">
      <h1 className="знак">CryVPN</h1>

      {шаг === ШАГ_ПОЧТА ? (
        <form className="поля" onSubmit={выслатьКод}>
          <p className="строка">Войдите той же почтой, что и на сайте</p>
          <input
            className="поле"
            type="email"
            inputMode="email"
            autoFocus
            placeholder="почта@example.com"
            value={почта}
            onChange={(e) => установитьПочту(e.target.value)}
            required
          />
          <button className="кнопка" disabled={занят}>
            {занят ? "Отправляем…" : "Получить код"}
          </button>
        </form>
      ) : (
        <form className="поля" onSubmit={войти}>
          <p className="строка">Код отправлен на {почта}</p>
          <input
            className="поле код"
            inputMode="numeric"
            autoFocus
            placeholder="— — — —"
            value={код}
            onChange={(e) => установитьКод(e.target.value)}
            required
          />
          <button className="кнопка" disabled={занят}>
            {занят ? "Проверяем…" : "Войти"}
          </button>
          <button
            type="button"
            className="кнопка второстепенная"
            onClick={() => {
              установитьШаг(ШАГ_ПОЧТА);
              установитьОшибку("");
            }}
          >
            Другая почта
          </button>
        </form>
      )}

      {ошибка && <p className="беда">{ошибка}</p>}
    </main>
  );
}
