import { defineAsyncComponent, markRaw, type Component } from "vue";
import {
  CodeOutline,
  PulseOutline,
  SwapVerticalOutline,
  TimeOutline,
} from "@vicons/ionicons5";

/**
 * 工具箱注册表：新增工具 = 在 tools/ 放一个组件 + 在这里注册一项，
 * 首页卡片/列表/搜索自动生效（ToolboxView 驱动）。
 */
export interface ToolboxTool {
  /** 唯一 id，用于选中态持久化（gw-toolbox-active）。 */
  id: string;
  title: string;
  /** 卡片上的功能简述。 */
  description: string;
  /** 搜索关键词（小写匹配，可放英文/拼音/别名）。 */
  keywords: string[];
  icon: Component;
  component: Component;
}

export const TOOLS: ToolboxTool[] = [
  {
    id: "port-checker",
    title: "端口查询",
    description: "查询端口占用进程（PID / 可执行路径），可一键终止释放端口",
    keywords: ["port", "端口", "占用", "进程", "pid", "kill"],
    icon: markRaw(PulseOutline),
    component: defineAsyncComponent(() => import("./tools/PortCheckerTool.vue")),
  },
  {
    id: "json-format",
    title: "JSON 格式化",
    description: "格式化 / 压缩 / 按 key 递归排序，非法 JSON 给出解析错误",
    keywords: ["json", "格式化", "压缩", "排序", "format", "minify", "sort"],
    icon: markRaw(CodeOutline),
    component: defineAsyncComponent(() => import("./tools/JsonFormatTool.vue")),
  },
  {
    id: "timestamp",
    title: "时间戳转换",
    description: "Unix 秒/毫秒与日期时间互转，当前时间实时刷新",
    keywords: ["timestamp", "时间戳", "日期", "date", "unix", "时间"],
    icon: markRaw(TimeOutline),
    component: defineAsyncComponent(() => import("./tools/TimestampTool.vue")),
  },
  {
    id: "base64",
    title: "Base64 编解码",
    description: "Base64 编码 / 解码，支持 UTF-8 中文内容",
    keywords: ["base64", "编码", "解码", "encode", "decode", "加密"],
    icon: markRaw(SwapVerticalOutline),
    component: defineAsyncComponent(() => import("./tools/Base64Tool.vue")),
  },
];
