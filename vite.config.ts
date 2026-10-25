import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error 未安装 @types/node 时的已知类型缺失
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],

  // 为 Tauri 定制的 Vite 选项，仅在 `tauri dev` / `tauri build` 时生效
  //
  // 1. 防止 Vite 清屏遮盖 Rust 错误输出
  clearScreen: false,
  // 2. Tauri 要求固定端口，被占用时直接失败
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. 让 Vite 忽略监听 `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
