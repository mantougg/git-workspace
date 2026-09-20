# F-50 AI 凭证固定使用加密文件存储（~/.gitworkspace/credentials），弃用 OS 凭证存储

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | 🟦 修复中 |
| 来源 | 2026-09-20 用户反馈：「凭证保存到 Windows 的 OS 凭证存储失败，这种兼容性问题可能有点大，永久保存使用 .gitworkspace 文件夹吧，不使用系统的凭证了，就像 .claude、.codex 那样，将 apikey 通过文件的方式保存，保存时使用加密，读取时解密」 |
| 关联任务 | 无（PAF-21 的 keyring 晚解锁逻辑随 OS 存储退化为迁移源而简化） |

## 问题描述

Windows 上写入 OS Credential Manager 失败，且失败路径有缺陷：
`CredentialManager::set`（`ai/credentials.rs:464`）只有 `Unavailable` 错误才
回退到加密文件（:477）；Windows 特有的非 Unavailable 失败（条目名/用户名
超长、blob 上限 2560 字节、凭证管理器损坏等，keyring 归为 `Other`）在 :481
直接报错拒绝——用户看到「保存失败」而不是静默回退。

用户决策：**永久保存固定使用加密文件**（对齐 `.claude`/`.codex` 的习惯），
不再依赖系统凭证。

## 现状资产

加密文件后端**已存在且测试完备**：`FileCredentialStore`
（`ai/credentials.rs:279`，XChaCha20-Poly1305 + Argon2id 派生密钥，每条凭证
独立 nonce），当前落盘在 `<app_data_dir>/credentials/`（:292，Windows =
`%APPDATA%/com.gitworkspace.app/credentials`）。

## 修复范围

- [x] 落盘目录改为**用户主目录** `~/.gitworkspace/credentials/`（对齐
      `.claude`/`.codex`；不放工作区级 `.gitworkspace/`——凭证是全局的，
      且项目内 `.gitworkspace/` 已被工作区级 runtimes/environments 占用）。
      `canonical_file_store_dir()`，home 不可用时回退 app_data_dir
- [x] `CredentialManager::production()` 装配改为：文件存储为唯一持久落点，
      会话内存兜底不变；OS 存储仅作**一次性迁移源**——`get` 时文件未命中则
      试 OS，命中即写入文件并删除 OS 条目（读迁移，用户无感）
- [x] `set(persist=true)` 只写文件（返回 `CredentialLocation::FileStore`）；
      旧位置 `<app_data_dir>/credentials/` 存在的文件启动时搬迁到新目录
      （`FileCredentialStore::migrate_from`，目标已存在则跳过）
- [x] UI 文案：`AiCredentialsSection`（加密文件说明 + 「已加密保存」标签）、
      `AiPrivacySection`（加密文件存储 + 安全边界说明）、`AiUsageSection`、
      `AiSettingsView` 工具栏标签；IPC 字段 `osCredentialStoreAvailable` →
      `persistentStoreAvailable`、`osStoreAvailable` → `fileStoreAvailable`
- [x] 测试更新：OS 优先断言改为文件优先；新增存量 OS 迁移、旧目录搬迁、
      规范目录位置用例；`os_credential_store_smoke_or_skip` 改为只读迁移源
      冒烟（`legacy_os_migration_source_smoke_or_skip`）

安全边界（已写入 UI 说明）：加密密钥由代码内固定材料派生
（`FILE_STORE_APP_SECRET`），防护等级是「不落明文」，与 `.claude`/`.codex`
的明文存储同级以上，但防不住同时拿到文件与程序本体的本机攻击者。

## 验收标准

- [x] 保存 API Key 后 `~/.gitworkspace/credentials/<ref>.json` 存在且为
      密文（nonce+ciphertext JSON），重启应用后读取正常（单测覆盖往返）
- [x] Windows 上不再触碰 Windows Credential Manager 写入路径（`set` 只写文件）
- [x] 存量 OS 凭证用户升级后首次读取自动迁移到文件（单测覆盖）
- [x] `GW_TEST_MANIFEST=1 cargo test --lib -- ai::credentials` 全绿
      （50/50 含 gateway/session 测试）；`vue-tsc` 通过

## 进度

### 状态

- 当前状态：✅ 已完成（代码 + 单测 + 类型检查全绿；真机保存/重启读取待用户实测）
- 最近更新：2026-09-20 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；定位：`set` 仅对 `Unavailable` 回退文件，Windows 特有 `Other` 失败直接拒绝；用户决策固定用加密文件 |
| 2026-09-20 | 🟦 | 开始修复 |
| 2026-09-20 | ✅ | 修复完成：文件存储为唯一持久落点（`~/.gitworkspace/credentials/`），OS 存储降为只读一次性迁移源（`get` 命中即写文件删 OS），旧 app_data_dir 目录启动搬迁；IPC/TS/UI 字段与文案同步；测试改写为文件优先 + 迁移用例，50/50 全绿，vue-tsc 通过 |
