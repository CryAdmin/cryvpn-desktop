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
