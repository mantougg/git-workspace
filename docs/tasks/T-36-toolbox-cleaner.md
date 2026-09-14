# T-36 工具箱·工作区清理（安全删除）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。
> 任务来源：用户需求（Roadmap 之外增补）——把独立 Go 工具 `D:\Code\PowerShell\delete-files`
> （delete-targets）的能力内置为工具箱小工具，配置模型 1:1 迁移。

| 项 | 值 |
|---|---|
| 阶段 | 用户需求增补 |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | — |
| 对应 Roadmap | 无（用户需求；落点遵循 D-XX 工具箱约定与 §46/§47 操作安全） |

## 目标

在工具箱内置「工作区清理」工具：配置**父路径列表 + 匹配规则（精确名/通配符 + 文件/目录类型）+ 排除目录**，
先扫描预览（含目录大小），输入 `DELETE` 确认后安全删除——替代手写 JSON 配置的 Go 独立工具。

## 需求范围

- [x] 配置：父路径列表（支持从已有 workspace 一键添加）、排除目录列表、规则列表
  （匹配方式：精确名 / 通配符；类型：目录 / 文件 / 文件和目录）；配置持久化（前端 localStorage）
- [x] 常用规则预设（node_modules / target / bin / obj / dist / build / .gradle / __pycache__ / *.tmp / *.log 一键加入）
- [x] 扫描预览：单次遍历匹配所有规则，进度事件推送（节流 100ms）；结果表格（路径 / 类型 / 大小）
- [x] 目录大小异步计算（文件大小扫描时直出，目录后台线程递归统计并增量推送）
- [x] 删除：前端输入 `DELETE`（prompt pattern 校验）+ 后端 `confirmed` 参数门禁 +
  服务端扫描会话 token（删除只认最近扫描结果，不接受前端任意路径）；支持勾选子集删除
- [x] 结果展示：成功 / 失败（含原因）分项汇总
- [ ] 正则匹配方式（候补，一期只做精确名 + 通配符）
- [ ] 扫描 / 大小计算取消（候补）

## 架构 / 性能注意点

- **单次遍历**（同 Go 版）：一次目录遍历同时匹配全部规则；命中可删目录后不深入其内部
  （内含排除路径的目录除外——不整体删除、继续扫描内部）；遍历结束剔除嵌套候选（父项优先）。
- **不跟随符号链接**：`DirEntry::file_type()`（Windows 零额外 syscall），symlink 目录视为
  非目录、不进栈递归（同 Go `isDir && !ModeSymlink`）。
- **路径处理**（平台规范 §1）：
  - 父路径经 `fs::canonicalize` 解析符号链接并绝对化（同 Go `EvalSymlinks`）；Windows 上产物为
    verbatim（`\\?\`）路径，内部流转天然规避 260 字符上限（node_modules 深路径场景）；
    展示时经 `pathutil::strip_windows_verbatim_prefix` 剥离。
  - 比较一律走 `pathutil::normalize_for_compare`（剥 verbatim + 分隔符统一 + 平台大小写折叠）。
    与 Go 版差异：Go 全平台小写化，本项目遵循平台规范（Linux 大小写敏感）——已在平台规范允许的
    「集合/去重语义可小写化」边界内，Windows/macOS 折叠、Linux 保留。
- **文件删除语义**：无全局开关（Go 版 `AllowFileDelete` 全局字段不做）；规则 `kind` 含文件
  （文件 / 文件和目录）即视为该规则显式允许删文件（等价 Go 规则级 `AllowFileDelete: true`）。
  目录规则绝不删文件。
- **模式守卫**：拒绝空 / 含路径分隔符 / 绝对路径 / `.`、`..` 的规则名；拒绝裸 `*` 通配符
  （防全量误删）。通配符仅实现 `*`（任意串）与 `?`（单字符），不实现字符类 `[...]`（一期边界）。
- **安全门禁（删除时逐条复检，同 Go 执行阶段）**：磁盘根目录禁删；应用 exe 目录与 app data
  目录（DB 所在）禁删（命中或目录内含均拒绝）；排除路径复检（命中排除 / 目录内含排除均拒绝）；
  候选必须来自本会话（token 校验）。
- **会话模型**（同 Go GUI）：后端内存保存最近一次扫描会话（token + 候选 + 排除清单）；
  删除成功后清会话（二次执行需重新扫描）；大小计算线程按 token 校验，防止跨会话串数据。
- **执行**：`remove_dir_all` / `remove_file`；只读文件先尝试去只读位再删（同 Go chmod +0200）；
  单项失败不中断，收集原因汇总。
- **进度/大小事件**：`cleaner-scan-progress`（节流 100ms + 完成帧）、`cleaner-size-progress`
  （逐项 + 完成帧），payload 带 token，前端按 token 过滤过期事件。
- 只读文件处理、verbatim 前缀为 Windows 特有行为，经 `cfg` 或运行时路径形态处理，测试用
  `std::env::temp_dir()` fixture（平台规范 §5）。

## 验收标准

- [x] 配置（父路径 / 排除目录 / 规则）可编辑并持久化，重新打开工具后恢复
- [x] 扫描预览正确命中规则目标；排除目录内目标不出现；命中目录不重复列其子项
- [x] 目录规则不删文件；文件规则只删其匹配的文件
- [x] 删除必须：最近扫描会话 + 前端 `DELETE` 输入 + 后端 confirmed；三项缺一即拒绝
- [x] 磁盘根目录 / 应用目录 / 排除路径在执行期复检拒绝（有单测覆盖）
- [x] 删除结果含成功 / 失败（原因）清单；失败不中断
- [x] `GW_TEST_MANIFEST=1 cargo test --lib` 通过（cleaner 19/19 全绿）
- [x] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-14 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-14 | 🟦 | 开始开发。设计定稿：配置模型 1:1 迁移 Go 版（父路径 / 规则 / 排除目录），会话 token + DELETE 确认 + confirmed 门禁三重防护；规则一期做精确名 + 通配符，正则候补；落点 `src-tauri/src/cleaner.rs`（核心+单测）+ `commands/cleaner.rs`（会话/事件）+ 工具箱注册表 + `WorkspaceCleanerTool.vue` |
| 2026-09-14 | ✅ | 完成。后端 `cleaner.rs` 核心（规则编译 / 单次遍历扫描 / 根目录·受保护·排除三重门禁 / 删除 / 目录大小）+ 19 项单测全绿；命令层会话 token + `confirmed` 门禁 + 子集删除 + 一次性会话；前端工具页（NDynamicSelect 路径列表 + 规则编辑 + 预设 + NDataTable 虚拟滚动预览 + DELETE prompt）。验证：`GW_TEST_MANIFEST=1 cargo test --lib` cleaner 19/19 通过（全量 908 过 / 16 失败均为存量：real-maven/vite 环境集成 + node::workspace + pathutil 大小写 + pty 回收，已 stash 基线复跑证实与本次改动无关）、`pnpm build`（vue-tsc + vite）通过 |

### 子任务清单

- [x] 任务文档 + README 同步
- [x] Rust 核心模块（规则编译 / 扫描 / 安全门禁 / 删除）+ 单测
- [x] Tauri 命令与会话状态（scan / sizes / execute）+ 注册
- [x] 前端 api 封装 + 工具页 + 注册表 + localStorage 持久化
- [x] 构建与测试验证
