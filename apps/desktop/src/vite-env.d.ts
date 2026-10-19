/// <reference types="vite/client" />

/**
 * Vite 客户端环境类型入口。
 *
 * 引用 vite/client 以获得静态资源模块声明（client.d.ts 里
 * `declare module '*.css' {}` 等）。TypeScript 6 起默认开启
 * 副作用导入检查（noUncheckedSideEffectImports），缺少这些声明时
 * `import "./styles/tokens.css"` 会报 TS2882。
 */
