import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// 开发服务器固定端口：Electron 主进程（electron/main.ts）在 dev 模式下
// 加载 127.0.0.1:5173；端口一变，窗口就会白屏。
export default defineConfig({
  // 相对 base：Electron 生产环境用 file:// 加载 dist/index.html，
  // 绝对路径 /assets/... 在 file:// 下会解析到盘符根目录而 404。
  base: "./",
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
      // Electron 主进程由 esbuild 单独打包，不需要 Vite watch
      ignored: ["**/electron/**"],
    },
  },
  envPrefix: ["VITE_"],
  build: {
    // Electron 45 自带 Chromium 156；这里仍保守地降到 chrome110，
    // 让语法降级有余量，也便于在普通浏览器里打开 dev server 调试。
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