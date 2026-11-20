import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Полоса from "./Полоса.jsx";
import ОбноваТелефон from "./ОбноваТелефон.jsx";
import "./сон.js";
import "./пульт.js";

// Системную полосу окна убираем в Windows и Linux; на Mac она и так тёмная.
// В Windows она белая, в Linux — какая выпадет у рабочего стола: синяя,
// серая, светлая. Своя полоса везде одна, в цвет окна.
const андроид = navigator.userAgent.includes("Android");
const айфон = /iPhone|iPad|iPod/.test(navigator.userAgent);
const телефон = андроид || айфон;
const своя_полоса =
  navigator.userAgent.includes("Windows") ||
  (navigator.userAgent.includes("Linux") && !телефон);
if (своя_полоса) document.documentElement.classList.add("с-полосой");
// На телефоне верх экрана сжимается при прокрутке списка, см. App.css.
if (телефон) document.documentElement.classList.add("телефон");
// Планшет с крупным масштабом экрана: у Honor Pad 10 (2560×1600) приложение
// видело всего около 700×400 точек, и всё выглядело огромным (20.11.2026).
// Если экран физически планшетный (короткая сторона от 1500 пикселей), а
// точек по длинной стороне меньше тысячи, ужимаем интерфейс до тысячи.
// Телефоны сюда не попадают: у них короткая сторона меньше 1500 пикселей.
if (андроид) {
  const плотность = window.devicePixelRatio || 1;
  const короткая_физ = Math.min(screen.width, screen.height) * плотность;
  const длинная = Math.max(screen.width, screen.height);
  if (короткая_физ >= 1500 && длинная < 1000) {
    const зум = Math.max(0.7, длинная / 1000);
    document.documentElement.style.zoom = String(зум);
    document.documentElement.style.setProperty("--зум", String(зум));
    document.documentElement.classList.add("ужат");
  }
}
// iPhone: окно заходит под полосу со временем и под полоску «домой»,
// отступы для них — в App.css.
if (айфон) {
  document.documentElement.classList.add("айфон");
  document
    .querySelector('meta[name="viewport"]')
    ?.setAttribute("content", "width=device-width, initial-scale=1.0, viewport-fit=cover");
}

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    {своя_полоса && <Полоса />}
    {андроид && <ОбноваТелефон />}
    <App />
  </React.StrictMode>,
);
