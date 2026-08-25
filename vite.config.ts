import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import AutoImport from "unplugin-auto-import/vite";
import Components from "unplugin-vue-components/vite";
import { ElementPlusResolver } from "unplugin-vue-components/resolvers";
import path from "path";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [
    vue(),
    // Element Plus 按需引入（构建体积优化）：模板中的 el-* 组件与
    // ElMessage/ElMessageBox 等 API 调用按需注册并自动引入对应样式；
    // 类型声明生成到 src/ 下（tsconfig include 范围内）。
    AutoImport({
      resolvers: [ElementPlusResolver()],
      dts: "src/auto-imports.d.ts",
    }),
    Components({
      resolvers: [ElementPlusResolver()],
      dts: "src/components.d.ts",
    }),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  build: {
    // 按需化后的 element-plus vendor chunk ≈ 590 kB（Tauri 桌面本地加载，
    // 属正常水位）；阈值从默认 500 kB 提到 700 kB，超出才告警。
    chunkSizeWarningLimit: 700,
    rollupOptions: {
      // element-plus 的 es 主入口顶层对每个组件调用 withInstall()（无 PURE
      // 注解），rollup 保守地将其视为副作用 → 全量打包。把安装函数标记为
      // PURE 后，未用组件的模块整条移除（它们仅在 app.use 全量注册时有
      // 副作用，本项目按需注册，不依赖该副作用）。
      treeshake: {
        manualPureFunctions: [
          "withInstall",
          "withNoopInstall",
          "withInstallDirectives",
          "withInstallDirective",
        ],
      },
      output: {
        // vendor 分包：按模块路径归集，只打包实际被按需引入的子模块
        //（对象形式的 manualChunks 会拉入包入口、抵消按需，勿用）。
        manualChunks(id: string) {
          if (id.includes("node_modules/element-plus")) return "element-plus";
          if (id.includes("node_modules/@element-plus/icons-vue"))
            return "element-plus-icons";
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
