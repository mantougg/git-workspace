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
          // Naive UI 按功能类别拆分，降低单 chunk 体积
          if (id.includes("node_modules/naive-ui/es/")) {
            // 数据展示：data-table、tree、pagination 等重量级组件
            if (
              id.includes("/data-table") ||
              id.includes("/tree") ||
              id.includes("/pagination") ||
              id.includes("/descriptions")
            )
              return "naive-data";
            // 表单组件：input、select、form、checkbox、radio 等
            if (
              id.includes("/input") ||
              id.includes("/select") ||
              id.includes("/form") ||
              id.includes("/checkbox") ||
              id.includes("/radio") ||
              id.includes("/switch") ||
              id.includes("/date-picker") ||
              id.includes("/input-number")
            )
              return "naive-form";
            // 反馈组件：dialog、modal、drawer、message、tooltip 等
            if (
              id.includes("/dialog") ||
              id.includes("/modal") ||
              id.includes("/drawer") ||
              id.includes("/message") ||
              id.includes("/tooltip") ||
              id.includes("/popconfirm") ||
              id.includes("/popover") ||
              id.includes("/notification") ||
              id.includes("/alert")
            )
              return "naive-feedback";
            // 布局与导航：card、tabs、collapse、steps、dropdown 等
            if (
              id.includes("/card") ||
              id.includes("/tabs") ||
              id.includes("/collapse") ||
              id.includes("/steps") ||
              id.includes("/dropdown") ||
              id.includes("/space") ||
              id.includes("/scrollbar") ||
              id.includes("/grid") ||
              id.includes("/divider")
            )
              return "naive-layout";
            // 剩余基础组件（button、tag、icon、spin 等）+ 内部工具
            return "naive-core";
          }
          if (id.includes("node_modules/naive-ui")) return "naive-core";
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
