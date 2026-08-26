import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import AutoImport from "unplugin-auto-import/vite";
import Components from "unplugin-vue-components/vite";
import { NaiveUiResolver } from "unplugin-vue-components/resolvers";
import path from "path";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
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
    // Naive UI 按需导入体积更小，阈值恢复到合理水平。
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        // vendor 分包：按模块路径归集，只打包实际被按需引入的子模块。
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
