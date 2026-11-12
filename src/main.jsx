import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Полоса from "./Полоса.jsx";
import ОбноваТелефон from "./ОбноваТелефон.jsx";

// Системную полосу окна убираем только в Windows; на Mac она и так тёмная.
const своя_полоса = navigator.userAgent.includes("Windows");
if (своя_полоса) document.documentElement.classList.add("с-полосой");
// На телефоне верх экрана сжимается при прокрутке списка, см. App.css.
const телефон = navigator.userAgent.includes("Android");
if (телефон) document.documentElement.classList.add("телефон");

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    {своя_полоса && <Полоса />}
    {телефон && <ОбноваТелефон />}
    <App />
  </React.StrictMode>,
);
