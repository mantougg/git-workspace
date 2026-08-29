import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import AutoImport from "unplugin-auto-import/vite";
import Components from "unplugin-vue-components/vite";
import { NaiveUiResolver } from "unplugin-vue-components/resolvers";
import path from "path";
import pkg from "./package.json";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  define: {
    // F-07：版本号/作者单一数据源是 package.json，构建期注入全局常量。
    __APP_VERSION__: JSON.stringify(pkg.version),
    __APP_AUTHOR__: JSON.stringify(pkg.author),
    __APP_LICENSE__: JSON.stringify(pkg.license),
    __APP_REPOSITORY__: JSON.stringify(pkg.repository),
  },
  plugins: [
    vue(),
    // Naive UI 按需引入：模板中的 n-* 组件与 useMessage/useDialog 等
    // composable 按需注册并自动引入对应样式；
    // 类型声明生成到 src/ 下（tsconfig include 范围内）。
    AutoImport({
      resolvers: [NaiveUiResolver()],
      dts: "src/auto-imports.d.ts",
    }),
    Components({
      resolvers: [NaiveUiResolver()],
      dts: "src/components.d.ts",
    }),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  build: {
    // Naive UI 按需引入后仍有 ~860 kB（内部交叉依赖无法拆分），Tauri 桌面应用无需过度关注体积。
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      output: {
        // vendor 分包：按模块路径归集，只打包实际被按需引入的子模块。
        // naive-ui 内部组件交叉依赖严重，拆分会产生循环 chunk，保持单包。
        manualChunks(id: string) {
          if (id.includes("node_modules/naive-ui")) return "naive-ui";
          if (id.includes("node_modules/@vicons")) return "vicons";
          if (
            id.includes("node_modules/vue/") ||
            id.includes("node_modules/vue-router") ||
            id.includes("node_modules/pinia") ||
            id.includes("node_modules/@vue/")
          )
            return "vue";
        },
      },
    },
  },
  // Tauri expects a fixed port, if this port is not available Tauri will error
  clearScreen: false,
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
      // Tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
