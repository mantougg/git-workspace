/// <reference types="vite/client" />

/** F-07：构建期从 package.json 注入（见 vite.config.ts define）。 */
declare const __APP_VERSION__: string;
declare const __APP_AUTHOR__: string;

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<Record<string, never>, Record<string, never>, any>;
  export default component;
}
