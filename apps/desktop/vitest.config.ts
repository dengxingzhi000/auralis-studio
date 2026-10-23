import { defineConfig } from "vitest/config";
import path from "node:path";

/**
 * 前端单元测试配置。
 *
 * ## 为什么单独一个文件而不是复用 vite.config.ts
 *
 * `vite.config.ts` 里挂着 React 插件与 Electron 开发服务器的固定端口设置。
 * 测试既不需要插件，也不想因为改了 dev server 端口而让 `npm test` 挂掉。
 * 这里只保留测试真正需要的东西：源码目录别名。
 *
 * ## environment: "node"
 *
 * 本项目要测的是**纯逻辑**：偏好校验、视口换算、历史记录、搜索匹配。
 * 这些都不需要 DOM。用 node 环境比 jsdom 轻一个数量级，也避免了
 * 「测试里能跑、浏览器里行为不同」的假象——被测代码本来就不碰 DOM。
 *
 * 少数组件（`MetricsPanel`）的**渲染输出本身**就是正确性契约
 * （`null` 必须渲染成「—」而不是 0，实时态不能混入文件读数）。
 * 这类组件没有 hooks/DOM 依赖，可以直接用 `react-dom/server` 的
 * `renderToStaticMarkup` 断言 HTML，不需要引入 jsdom + Testing Library
 * 那一整套新基建。需要 DOM 的交互仍靠人工验收清单覆盖。
 */
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.{ts,tsx}"],
  },
  resolve: {
    // 与 vite.config.ts 保持一致的别名，测试里的 import 才能用 "@/lib/..."。
    alias: { "@": path.resolve(import.meta.dirname, "./src") },
  },
});
