import { defineAsyncComponent, markRaw, type Component } from "vue";
import {
  BookOutline,
  CodeOutline,
  GitNetworkOutline,
  KeyOutline,
  LinkOutline,
  LockClosedOutline,
  PulseOutline,
  SwapVerticalOutline,
  TimeOutline,
  ShieldCheckmarkOutline,
  TextOutline,
  CodeSlashOutline,
  LanguageOutline,
  GlobeOutline,
  CalculatorOutline,
  CalendarOutline,
  InformationCircleOutline,
  DocumentTextOutline,
  ColorPaletteOutline,
} from "@vicons/ionicons5";

/**
 * 工具箱注册表：新增工具 = 在 tools/ 放一个组件 + 在这里注册一项，
 * 首页卡片/列表分组/搜索自动生效（ToolboxView 驱动）。
 */
export interface ToolboxTool {
  /** 唯一 id。 */
  id: string;
  title: string;
  /** 卡片上的功能简述。 */
  description: string;
  /** 类型分组（左侧列表按此分组，须是 TOOL_CATEGORIES 之一）。 */
  category: ToolCategory;
  /** 搜索关键词（小写匹配，可放英文/拼音/别名）。 */
  keywords: string[];
  icon: Component;
  component: Component;
}

/** 类型分组与展示顺序。 */
export const TOOL_CATEGORIES = [
  "网络",
  "编码解码",
  "格式转换",
  "时间与生成",
  "速查表",
  "计算器",
] as const;

export type ToolCategory = (typeof TOOL_CATEGORIES)[number];

export const TOOLS: ToolboxTool[] = [
  // ── 网络 ──
  {
    id: "port-checker",
    title: "端口查询",
    description: "查询端口占用进程（PID / 可执行路径），可一键终止释放端口",
    category: "网络",
    keywords: ["port", "端口", "占用", "进程", "pid", "kill"],
    icon: markRaw(PulseOutline),
    component: defineAsyncComponent(() => import("./tools/PortCheckerTool.vue")),
  },
  {
    id: "route-split",
    title: "路由分流",
    description: "双网卡分流：内网网段走网线、其余流量走 WiFi/热点（Windows，需管理员）",
    category: "网络",
    keywords: ["route", "路由", "分流", "双网卡", "内网", "外网", "wifi", "metric"],
    icon: markRaw(GitNetworkOutline),
    component: defineAsyncComponent(() => import("./tools/RouteSplitTool.vue")),
  },
  {
    id: "ip-address",
    title: "IP 地址查询",
    description: "查询 IP 地址的地理位置、ISP 信息，支持查询本机公网 IP",
    category: "网络",
    keywords: ["ip", "地址", "查询", "地理位置", "isp", "公网", "本机"],
    icon: markRaw(GlobeOutline),
    component: defineAsyncComponent(() => import("./tools/IpAddressTool.vue")),
  },
  {
    id: "subnet-calculator",
    title: "子网计算器",
    description: "计算子网掩码、网络地址、广播地址、可用主机数，支持子网划分",
    category: "网络",
    keywords: ["subnet", "子网", "掩码", "cidr", "网络", "广播", "计算"],
    icon: markRaw(CalculatorOutline),
    component: defineAsyncComponent(() => import("./tools/SubnetCalculatorTool.vue")),
  },
  // ── 编码解码 ──
  {
    id: "base64",
    title: "Base64 编解码",
    description: "Base64 编码 / 解码，支持 UTF-8 中文内容",
    category: "编码解码",
    keywords: ["base64", "编码", "解码", "encode", "decode", "加密"],
    icon: markRaw(SwapVerticalOutline),
    component: defineAsyncComponent(() => import("./tools/Base64Tool.vue")),
  },
  {
    id: "url-codec",
    title: "URL 编解码",
    description: "URL 参数编码 / 解码（encodeURIComponent），处理转义字符",
    category: "编码解码",
    keywords: ["url", "编码", "解码", "encode", "decode", "uri", "转义"],
    icon: markRaw(LinkOutline),
    component: defineAsyncComponent(() => import("./tools/UrlCodecTool.vue")),
  },
  {
    id: "jwt-parser",
    title: "JWT 解析器",
    description: "解析 JWT Token，查看 Header、Payload 和过期时间",
    category: "编码解码",
    keywords: ["jwt", "token", "解析", "parse", "header", "payload", "过期"],
    icon: markRaw(ShieldCheckmarkOutline),
    component: defineAsyncComponent(() => import("./tools/JwtParserTool.vue")),
  },
  {
    id: "html-entity",
    title: "HTML 实体编解码",
    description: "HTML 实体编码 / 解码，支持常用实体和数字实体",
    category: "编码解码",
    keywords: ["html", "实体", "编码", "解码", "entity", "转义", "special"],
    icon: markRaw(CodeSlashOutline),
    component: defineAsyncComponent(() => import("./tools/HtmlEntityTool.vue")),
  },
  {
    id: "unicode-codec",
    title: "Unicode 编解码",
    description: "Unicode / HTML / CSS / JS 编解码，查看字符详细信息",
    category: "编码解码",
    keywords: ["unicode", "编码", "解码", "html", "css", "js", "字符", "codepoint"],
    icon: markRaw(LanguageOutline),
    component: defineAsyncComponent(() => import("./tools/UnicodeCodecTool.vue")),
  },
  // ── 格式转换 ──
  {
    id: "json-format",
    title: "JSON 格式化",
    description: "格式化 / 压缩 / 按 key 递归排序，非法 JSON 给出解析错误",
    category: "格式转换",
    keywords: ["json", "格式化", "压缩", "排序", "format", "minify", "sort"],
    icon: markRaw(CodeOutline),
    component: defineAsyncComponent(() => import("./tools/JsonFormatTool.vue")),
  },
  {
    id: "regex-tester",
    title: "正则表达式测试器",
    description: "实时测试正则表达式，高亮显示匹配结果和捕获组",
    category: "格式转换",
    keywords: ["regex", "正则", "表达式", "匹配", "测试", "pattern"],
    icon: markRaw(TextOutline),
    component: defineAsyncComponent(() => import("./tools/RegexTesterTool.vue")),
  },
  // ── 时间与生成 ──
  {
    id: "timestamp",
    title: "时间戳转换",
    description: "Unix 秒/毫秒与日期时间互转，当前时间实时刷新",
    category: "时间与生成",
    keywords: ["timestamp", "时间戳", "日期", "date", "unix", "时间"],
    icon: markRaw(TimeOutline),
    component: defineAsyncComponent(() => import("./tools/TimestampTool.vue")),
  },
  {
    id: "ulid-nanoid",
    title: "ULID / NanoID 生成",
    description: "生成 ULID（有序、时间可解）与 NanoID（URL 安全），支持批量",
    category: "时间与生成",
    keywords: ["ulid", "nanoid", "uuid", "id", "生成", "随机"],
    icon: markRaw(KeyOutline),
    component: defineAsyncComponent(() => import("./tools/UlidNanoidTool.vue")),
  },
  {
    id: "cron-generator",
    title: "Cron 表达式生成器",
    description: "可视化生成 Cron 表达式，预览下次执行时间",
    category: "时间与生成",
    keywords: ["cron", "定时", "任务", "表达式", "生成", "调度"],
    icon: markRaw(CalendarOutline),
    component: defineAsyncComponent(() => import("./tools/CronGeneratorTool.vue")),
  },
  {
    id: "lorem-ipsum",
    title: "Lorem Ipsum 生成",
    description: "生成占位文本，支持拉丁文、中文、英文，可生成段落、句子、单词",
    category: "时间与生成",
    keywords: ["lorem", "ipsum", "占位", "文本", "生成", "随机", "测试"],
    icon: markRaw(DocumentTextOutline),
    component: defineAsyncComponent(() => import("./tools/LoremIpsumTool.vue")),
  },
  // ── 速查表 ──
  {
    id: "http-status",
    title: "HTTP 状态码 / Header 速查",
    description: "常用状态码与请求/响应头的含义速查，支持搜索",
    category: "速查表",
    keywords: ["http", "状态码", "header", "响应头", "请求头", "status"],
    icon: markRaw(BookOutline),
    component: defineAsyncComponent(() => import("./tools/HttpStatusTool.vue")),
  },
  {
    id: "git-cheatsheet",
    title: "Git 命令速查",
    description: "按场景查常用 Git 命令（撤销/分支/暂存/变基…），一键复制",
    category: "速查表",
    keywords: ["git", "命令", "速查", "cheatsheet", "撤销", "变基"],
    icon: markRaw(GitNetworkOutline),
    component: defineAsyncComponent(() => import("./tools/GitCheatSheetTool.vue")),
  },
  {
    id: "css-cheatsheet",
    title: "CSS 速查表",
    description: "常用 CSS 属性、值和代码片段速查，支持搜索和分类",
    category: "速查表",
    keywords: ["css", "属性", "速查", "cheatsheet", "样式", "布局", "flex", "grid"],
    icon: markRaw(ColorPaletteOutline),
    component: defineAsyncComponent(() => import("./tools/CssCheatSheetTool.vue")),
  },
  // ── 计算器 ──
  {
    id: "chmod",
    title: "chmod 权限计算器",
    description: "rwx 勾选 ↔ 八进制数字 ↔ 符号表示互转，含常用预设",
    category: "计算器",
    keywords: ["chmod", "权限", "755", "rwx", "linux", "octal"],
    icon: markRaw(LockClosedOutline),
    component: defineAsyncComponent(() => import("./tools/ChmodTool.vue")),
  },
  {
    id: "system-info",
    title: "系统信息",
    description: "查看浏览器、操作系统、硬件、网络等系统信息",
    category: "计算器",
    keywords: ["system", "系统", "信息", "浏览器", "硬件", "网络", "os"],
    icon: markRaw(InformationCircleOutline),
    component: defineAsyncComponent(() => import("./tools/SystemInfoTool.vue")),
  },
];
