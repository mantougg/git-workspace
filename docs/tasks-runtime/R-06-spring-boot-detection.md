# R-06 Spring Boot 应用发现与 Main Class 推断

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-01 Maven 项目发现与 POM 解析](./R-01-maven-discovery.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | R-01 |
| 对应源文档 | §21 Spring Boot 应用发现、§22 Main Class |

## 目标

在已发现的 Maven 项目中自动识别 Spring Boot 应用，给出候选 Main Class 列表并自动推断默认值，作为创建 Runtime 配置（R-07）的入口。

## 需求范围

- [x] 检测 `spring-boot-maven-plugin`（pom plugins，含 parent 继承场景）（§21）
- [x] 检测 `@SpringBootApplication` 源码注解：轻量文本/正则扫描 `src/main/java`，**不建 Java AST / 代码索引**（全局约束 §1）
- [x] 候选 Main Class 列表：全限定类名 + 所在模块（如 `com.example.Application / AdminApplication / GatewayApplication`）
- [x] 默认 Main Class 自动推断（§22）：唯一候选直接选定；多候选按命名/插件配置启发式排序；pom `start-class` 属性优先
- [x] 非 Spring Boot 模块（纯 Library）不产生候选
- [x] 结果纳入 R-01 解析缓存一并失效/复用

## 架构 / 性能注意点

- 注解扫描只在「pom 含 spring-boot 依赖或插件」的模块内触发，避免全 workspace 扫源码。
- 文本扫描按模块粒度并行，单模块文件数设上限防御异常工程；扫描结果随 POM Cache 一同缓存。
- `@SpringBootApplication` 可能经 meta-annotation 间接引入——第一版只认直接注解，识别不到时允许用户手填 Main Class（配置侧兜底）。

## 验收标准

- [x] 样例工程候选 Main Class 列表完整，默认推断与 `mvn spring-boot:run` 实际启动类一致
- [x] 多候选场景（Admin + Gateway）全部列出且可区分模块
- [x] 纯 Library 模块不产生候选
- [x] pom/源码变化后候选列表正确刷新

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-18 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-18 | 🟦 开始开发 | 启动 Spring Boot 插件/依赖检测、源码注解扫描、Main Class 候选排序、内容指纹缓存与 IPC 类型接入 |
| 2026-08-18 | ✅ 完成 | 完成 Spring Boot Maven 插件/依赖及 workspace parent 继承检测、`@SpringBootApplication` 有界文本扫描、多候选排序与 `start-class` 优先推断；接入进程级 POM/源码指纹缓存、R-06 IPC 命令、TS 类型/API 与 golden 快照。验证：`cargo test --all-targets`（270 passed / 2 ignored）、`cargo check --all-targets`、`cargo clippy --all-targets --all-features`（仓库既有警告）、`pnpm build`、R-06 定向测试 5 passed |

### 子任务清单

- [x] spring-boot 插件/依赖判定
- [x] `@SpringBootApplication` 轻量扫描器
- [x] 候选收集与默认推断启发式
- [x] 缓存接入（随 R-01 失效）
- [x] 单元测试
