import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Полоса from "./Полоса.jsx";
import ОбноваТелефон from "./ОбноваТелефон.jsx";

// Системную полосу окна убираем в Windows и Linux; на Mac она и так тёмная.
// В Windows она белая, в Linux — какая выпадет у рабочего стола: синяя,
// серая, светлая. Своя полоса везде одна, в цвет окна.
const телефон = navigator.userAgent.includes("Android");
const своя_полоса =
  navigator.userAgent.includes("Windows") ||
  (navigator.userAgent.includes("Linux") && !телефон);
if (своя_полоса) document.documentElement.classList.add("с-полосой");
// На телефоне верх экрана сжимается при прокрутке списка, см. App.css.
if (телефон) document.documentElement.classList.add("телефон");

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    {своя_полоса && <Полоса />}
    {телефон && <ОбноваТелефон />}
    <App />
  </React.StrictMode>,
);
