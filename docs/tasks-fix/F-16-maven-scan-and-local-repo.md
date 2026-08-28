# F-16 Maven 可执行体扫描/手动添加 + 本地仓库路径可选

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成（2026-08-28） |
| 来源 | 2026-08-28 用户实测反馈问题 3 |
| 关联任务 | F-03（JDK 全量扫描同款能力）、R-05 Maven 检测链 |

## 问题描述

Maven 设置页只有「检测项目 Maven」（选项目目录跑 wrapper/配置/系统优先级链），
没有类似 JDK 管理的全量扫描能力：mise 下装了多个 Maven 版本无法发现入库，
也不能手动添加一个自定义 mvn 路径。另外「本地仓库路径」只读展示
（settings.xml 探测值），不能选择/覆盖。

## 定位线索

- 现有检测：`src-tauri/src/maven/detect_exec.rs::detect_maven_candidates`
  （wrapper/configured/PATH 三级），无 mise/安装目录扫描
- JDK 全量扫描参照：`src-tauri/src/java/detect.rs::discover_jdks` +
  `mise_homes()`（mise installs/java；Maven 对应 `installs/maven/<ver>/bin/mvn`）
- 注册表：`maven/registry.rs`（upsert / validate / prune 已就绪）
- 前端：`src/views/MavenSettingsView.vue`（工具行只有「清理失效条目」与
  「检测项目 Maven」）；`src/api/maven.ts`
- 本地仓库解析：`src-tauri/src/maven/settings.rs::resolve_local_repository`
  （settings.xml → ~/.m2/repository），UI 覆盖需要新增配置项并贯穿调用点

## 修复范围

- [x] 后端新增「扫描 Maven 安装」命令：mise installs/maven + PATH +
  常见安装目录，探测版本并入库（仿 JDK discover 的去重/有效性语义）
- [x] 后端新增「手动添加 Maven 路径」命令（校验可执行 + probe 版本 + 入库，
  source=configured）
- [x] Maven 设置页加「扫描安装」「手动添加」按钮（tokens/Panel 规范）
- [x] 本地仓库路径支持 UI 选择/覆盖（目录选择器；覆盖值持久化，优先级
  高于 settings.xml 探测）
- [x] 平台规范：Windows 可执行候选按 `.exe → .cmd → .bat` 顺序
  （复用 `find_in_path` 语义）

## 验收标准

- [x] 扫描能发现 mise 下的 Maven 安装（本机实测 mise 装有 maven）
- [x] 手动添加一个自定义 mvn 路径后出现在列表且可复检通过
- [x] 本地仓库路径可修改并持久化；命令预览使用覆盖后的路径
- [x] `cargo test maven` 不回归；`pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-28 修复完成并实测验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-28 | ⬜ | 问题录入（拆分自用户反馈） |
| 2026-08-28 | ✅ | 修复：`detect_exec.rs::scan_maven_installations`（mise installs/maven + SDKMAN candidates + PATH，PATHEXT 扩展名优先，路径归一化去重）；`settings.rs` 应用级本地仓库覆盖（`<app_data>/maven-settings.json`，优先级 应用覆盖 > settings.xml > 默认；pipeline/detect/resolve_local_repo 全切 `resolve_local_repository_effective`）；命令层新增 scan_maven_installations / add_maven_executable（probe `mvn -v` 后入库，失败给可行动错误）/ get|set_maven_local_repo_override；设置页加「扫描安装」「手动添加」按钮 + 本地仓库「选择目录…/清除覆盖」。验证：settings 新增单测（覆盖优先级/文件往返）过；安装包 CDP 实测「扫描安装」发现 mise maven 3.9.14（mvn.cmd 命中正确）并入库展示；`cargo test maven::` 81 过、real_maven（JDK17）11 过；`pnpm build` 过 |
