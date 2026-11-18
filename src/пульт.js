// Пульт телевизора: стрелками переходим по кнопкам и строкам, ОК нажимает.
//
// На Android TV приложение — та же сборка, что и на телефоне, только
// управляют им пультом. Стрелки пульта WebView отдаёт странице как
// клавиши, а сама страница умеет по ним только прокручиваться: до списка
// серверов пульт не доходил, и выбрать сервер было нельзя (жалоба
// 18.11.2026). Здесь стрелка переводит выделение на ближайший элемент в
// её сторону, а список сам прокручивается за выделением.
//
// Включается только на Android: на компьютере стрелки остаются как были.

const МОЖНО_ВЫБРАТЬ = 'button:not([disabled]), [tabindex="0"], input, select, a[href]';

// Окна поверх экрана: пока открыто такое окно, ходим только внутри него.
const СЛОИ = ".окошко, .меню-сервера, .выбор-туннеля";

const НАПРАВЛЕНИЯ = {
  ArrowUp: [0, -1],
  ArrowDown: [0, 1],
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
};

function видим(э) {
  const р = э.getBoundingClientRect();
  return р.width > 0 && р.height > 0 && getComputedStyle(э).visibility !== "hidden";
}

function центр(э) {
  const р = э.getBoundingClientRect();
  return [р.left + р.width / 2, р.top + р.height / 2];
}

/** Где сейчас ходить: в открытом окне, если оно есть, иначе по экрану. */
function область() {
  const окна = [...document.querySelectorAll(СЛОИ)].filter(видим);
  return окна[окна.length - 1] ?? document;
}

function выделить(э) {
  э.focus({ preventScroll: true });
  э.scrollIntoView({ block: "nearest", inline: "nearest" });
}

function шаг(е) {
  const куда = НАПРАВЛЕНИЯ[е.key];
  if (!куда) return;
  const сейчас = document.activeElement;
  // В поле ввода стрелки влево-вправо двигают курсор по тексту.
  if (сейчас?.tagName === "INPUT" && куда[0] !== 0) return;

  const где = область();
  const все = [...где.querySelectorAll(МОЖНО_ВЫБРАТЬ)].filter(видим);
  if (!все.length) return;
  е.preventDefault();

  // Ничего не выделено (или выделение осталось под закрытым окном) —
  // начинаем с главной кнопки, а в окне — с первого, что можно выбрать.
  if (!сейчас || сейчас === document.body || !где.contains(сейчас)) {
    выделить(где.querySelector(".круг") ?? все[0]);
    return;
  }

  const [x, y] = центр(сейчас);
  let лучший = null;
  let оценка = Infinity;
  for (const э of все) {
    if (э === сейчас) continue;
    const [ex, ey] = центр(э);
    const dx = ex - x;
    const dy = ey - y;
    // Вдоль направления — насколько далеко, поперёк — насколько в стороне.
    const вдоль = dx * куда[0] + dy * куда[1];
    if (вдоль <= 1) continue;
    const поперёк = Math.abs(dx * куда[1]) + Math.abs(dy * куда[0]);
    const счёт = вдоль + поперёк * 2;
    if (счёт < оценка) {
      оценка = счёт;
      лучший = э;
    }
  }
  if (лучший) выделить(лучший);
}

if (navigator.userAgent.includes("Android")) {
  document.addEventListener("keydown", шаг);
}
