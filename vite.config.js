import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Сборка интерфейса. Отладочные карты не выпускаем: с ними готовое
// приложение читается как открытый исходник. Это не защита от разбора —
// разобрать можно всё — но лишнюю подсказку взломщику мы не дарим.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "chrome110",
    sourcemap: false,
    reportCompressedSize: false,
  },
});
