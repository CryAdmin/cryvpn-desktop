import "flag-icons/css/flag-icons.min.css";

// Windows не умеет рисовать флаги-эмодзи: вместо 🇱🇻 показывает буквы «LV».
// Поэтому вытаскиваем код страны из названия и рисуем флаг картинкой —
// одинаково на любой системе и без зависимости от шрифтов.

const ПЕРВАЯ = 0x1f1e6; // 🇦
const ПОСЛЕДНЯЯ = 0x1f1ff; // 🇿

/** Код страны из флага-эмодзи в начале названия, иначе null. */
export function кодСтраны(имя = "") {
  const буквы = [];
  for (const знак of имя) {
    const код = знак.codePointAt(0);
    if (код >= ПЕРВАЯ && код <= ПОСЛЕДНЯЯ) {
      буквы.push(String.fromCharCode(код - ПЕРВАЯ + 65));
      if (буквы.length === 2) break;
    } else if (буквы.length) {
      break;
    }
  }
  if (буквы.length !== 2) return null;

  const код = буквы.join("").toLowerCase();
  // Флага Евросоюза в наборе нет, подменяем на звёздный круг Европы.
  return код === "eu" ? "eu" : код;
}

/** Название без флага-эмодзи: рисовать его текстом больше не нужно. */
export function безФлага(имя = "") {
  return имя
    .replace(/[\u{1F1E6}-\u{1F1FF}]{2}/gu, "")
    .replace(/️/g, "")
    .trim();
}

export default function Флаг({ имя }) {
  const код = кодСтраны(имя);
  if (!код) return <span className="флаг пусто" />;
  return <span className={`флаг fi fi-${код}`} />;
}
