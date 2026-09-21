# F-54 项目目录移动/重命名后 Runtime 启动报错误导 + 新建后端应用智能检测失效

> 状态：✅ 已完成
> 优先级：P0（用户实测阻塞：目录改名后所有后端应用无法恢复）
> 来源：2026-09-21 用户实测反馈（Runtime 总览菜单页）。

## 问题描述

用户把工作区 `D:\AWork\Code\IPD` 下的 `docs\03原型` 目录重命名为 `docs\04原型` 后：

1. **旧后端应用启动报错**：`在终端中启动失败：Project not found: Maven POM D:/AWork/Code/IPD/docs/03原型/hussar-web/pom.xml is missing（可重试）`。
2. **旧前端应用启动报错**：`在终端中启动失败：script Some("serve") not found in Node project D:/AWork/Code/IPD/docs/03原型/hussar-front（可重试）`。
3. **新建后端应用：智能检测不好使**（点「自动检测」提示"该项目未检测到 Spring Boot Main Class"），**新建后启动也报错**（同样的 POM missing）。
4. 新建前端应用可以正常启动。

## 根因定位（已核实，2026-09-21）

- 磁盘事实：`D:/AWork/Code/IPD/docs/` 下只有 `04原型`（含 `hussar-web/pom.xml`、`hussar-front/package.json`），`03原型` 已不存在；`hussar-web` 的 `@SpringBootApplication` 主类 `com.jxdinfo.hussar.example.HussarApplication` 在新路径源码里存在。
- DB 事实（`%APPDATA%/com.gitworkspace.app/gitworkspace.db`，workspace_id=7）：
  - `maven_projects` 索引停在 2026-09-18，仍是旧路径 `03原型/hussar-web/pom.xml`；
  - `node_projects` 已被实时扫描刷新（2026-09-21），含新路径 `04原型/hussar-front`；
  - 两个旧 Runtime 配置（`IPD原型前端` / `IDP启动后端`）的 `project` 仍指向 `03原型`。

错误链路：

1. **旧前端报错误导**：终端启动走 `runtimeComputeLaunchPreview` → `load_config_unredacted` → `validate_for_workspace`（`src-tauri/src/runtime/config/repository.rs:352`）。索引里没有旧路径、`read_scripts_from_disk` 读不到旧路径的 package.json → `scripts=None` → 落入 `ScriptNotFound` 且 `available=[]`（repository.rs:389）。真实问题是**项目目录不存在**，却报成"script 不存在"；且 `error.rs:69` 的 `script {script:?}` 把 Rust Debug 格式 `Some("serve")` 泄漏给用户。
2. **新建后端智能检测失效**：向导 Maven 下拉来自 DB 索引（`runtime_list_projects` / `store.projects`），只有旧路径条目；「自动检测」调 `detect_spring_boot`（`commands/spring_boot.rs`，实时扫描 workspace）返回的是**新路径**，`RuntimeAppWizard.vue::onDetectMainClass` 的路径匹配（旧路径 vs 新路径）失败 → "未检测到"。
3. **新建后报错**：用户只能用下拉里的旧路径条目创建 → 启动时 `require_project_pom`（`maven/reactor.rs:104`）检查 `project.path.is_file()` 失败 → "Maven POM ... is missing"。
4. **Node 不受影响的成因**：`runtime_list_unified_projects`（`commands/runtime.rs:183`）对 node 侧每次实时扫描（`discover_package_jsons`）并回写索引，maven 侧纯走 DB 索引——两侧新鲜度不对称。

恢复路径（用户侧）：重新「解析依赖」刷新 Maven 索引 → 编辑/重建应用到新路径。`maven/index/sync.rs::sync_workspace_index` 的 `delete_stale_projects` 会清掉旧路径，机制上无问题；缺陷在**引导与报错语义**。

## 修复范围 checklist

- [x] 1. `error.rs` `ScriptNotFound` Display 不再泄漏 `Some(...)`（`script 'serve' not found in Node project …`）。
- [x] 2. `repository.rs::validate_for_workspace`：Node 项目目录/package.json 不存在时返回 `ProjectNotFound`（"项目路径不存在，可能已被移动/重命名，请编辑应用重新选择项目"），不再误报 `ScriptNotFound`；package.json 存在但 script 缺失时维持 `ScriptNotFound`（带 available）。
- [x] 3. `runtime_list_unified_projects` 的 `UnifiedProjectNode` 增加 `pathExists: bool`（maven 按 pom.xml `is_file()`，node 按目录 `is_dir()`）；同步 `src/types/runtime.ts` 与 ipc_golden 快照（`models/ipc_golden/runtime.rs:338/1020`，golden 经 `GW_UPDATE_GOLDEN=1` 再生，diff 仅 +pathExists）。
- [x] 4. 向导 `RuntimeAppWizard.vue` Maven 下拉：改从 unified 列表取 maven 源；`pathExists=false` 的条目禁用 + 标注「路径不存在」；存在失效条目时显示警告 alert 并常显「解析依赖」按钮（原仅在空列表时显示）；`store.projects` 变化（dependencyResolved 事件驱动）时重拉 unified 列表。
- [x] 5. 向导「自动检测」：精确路径匹配失败时按 artifactId 唯一匹配兜底——命中且路径不同则预填 Main Class 并提示「索引路径已失效，请重新解析依赖」。

## 不做（范围控制）

- Runtime 总览行级「路径失效」标记（可作为后续增强单独提任务）。
- 自动把配置的 project 改写到新路径（涉及索引与配置一致性，由用户显式重新选择）。
- `pipeline/mod.rs:104` 的 package.json 读取错误消息维持现状（`RuntimeConfig`，语义已正确）。

## 验收标准

1. 指向已不存在目录的前端应用启动时，报 `ProjectNotFound`（消息含"项目路径不存在"与移动/重命名提示），**不再出现** `Some("serve")`。
2. `ScriptNotFound` 序列化消息不再含 `Some(`。
3. 向导 Maven 下拉对不存在路径的条目禁用并标注；有失效条目时可见「解析依赖」入口；解析完成后列表自动刷新。
4. 所选项目路径失效但检测结果中存在同 artifactId 项目时，「自动检测」能预填 Main Class 并提示索引需刷新。
5. `GW_TEST_MANIFEST=1 cargo test --lib` 通过；前端 `pnpm build`（含 vue-tsc）通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-21 | 开始修复。已复现并定位全链路（见「根因定位」）：03原型→04原型 目录改名 + maven 索引停留在 09-18 + node 实时刷新不对称。 |
| 2026-09-21 | 修复完成。根因：目录改名后 Maven 索引陈旧 + Node 校验把"目录不存在"误报为 ScriptNotFound（且 Display 泄漏 `Some(..)`）。修法：① ScriptNotFound Display 去掉 Debug 包装；② validate_for_workspace 先判 package.json 存在性，缺失报 ProjectNotFound（含移动/重命名提示）；③ UnifiedProjectNode 增加 pathExists；④ 向导 Maven 下拉禁用失效条目 + 常显「解析依赖」入口 + dependencyResolved 后自动刷新；⑤ 自动检测按 artifactId 唯一匹配兜底并提示刷新索引。验证：`GW_TEST_MANIFEST=1 cargo test --lib -- error:: runtime::config:: ipc_golden` 24 绿（含 3 个新增测试）；golden 再生 diff 仅 +pathExists；`pnpm build`（vue-tsc+vite）绿；全量 cargo test 其余 18 个失败经 stash 基线对照确认为既有环境问题（real-maven 网络/temp 路径 canonicalize/PTY 等），与本改动无关。实机回归待用户在 IPD 工作区复测：解析依赖 → 旧应用重选 04原型 路径 → 启动。 |
