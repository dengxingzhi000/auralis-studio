import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Tauri 通过固定端口的 dev server 连接，配置需与 tauri.conf.json 的
// devUrl 保持一致。
export default defineConfig({
  plugins: [react()],
  resolve: {
    // 用 import.meta.dirname 而非 __dirname：本项目是 "type": "module"，
    // __dirname 只是 Vite 打包式配置加载器注入的垫片。Vite 8 已警告它
    // 会随 configLoader: 'native' 成为默认而失效。
    alias: { "@": path.resolve(import.meta.dirname, "./src") },
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: "127.0.0.1",
    watch: {
      // Rust 侧由 cargo 自己监视，前端不需要 watch src-tauri
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    // Tauri 使用系统 WebView2，产物应针对较新的 Chromium。
    //
    // 注意：这里的 target 只约束 JS 语法降级与 CSS 语法降级，
    // 不约束 CSS 函数。源码 layout.css 里的 color-mix() 属于
    // Chrome 111+ 特性，实际运行时下限已经是 111 而非 110。
    //
    // 正因为 target 仍是真实浏览器版本，Vite 的 vite:esbuild-transpile
    // 插件不会跳过（它只在 target 为空/'esnext' 且 minify 不是 esbuild
    // 时返回 null），需要 esbuild 存在。这也是 esbuild 留在
    // devDependencies 里的唯一原因——它不参与压缩，只做降级转译。
    target: "chrome110",
    // Vite 8 起 JS 压缩器默认换成 Oxc。
    minify: "oxc",
    sourcemap: false,
    outDir: "dist",
    emptyOutDir: true,
  },
});