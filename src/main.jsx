import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Полоса from "./Полоса.jsx";

// Системную полосу окна убираем только в Windows; на Mac она и так тёмная.
const своя_полоса = navigator.userAgent.includes("Windows");
if (своя_полоса) document.documentElement.classList.add("с-полосой");

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    {своя_полоса && <Полоса />}
    <App />
  </React.StrictMode>,
);
