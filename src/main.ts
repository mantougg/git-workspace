import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import "./styles/tokens.scss";

const app = createApp(App);
app.use(createPinia());
app.use(router);
// Element Plus 按需注册由 unplugin-vue-components 在编译期完成（含样式），
// 不再全量 app.use(ElementPlus) / 引入全量 CSS。
app.mount("#app");
