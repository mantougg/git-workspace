# R-19 Runtime Templates

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-07 Runtime 配置体系](./R-07-runtime-config.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · 多服务与效率 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | R-07 |
| 对应源文档 | §83 Runtime Template |

## 目标

提供 Runtime 配置模板：内置常用模板 + 用户自定义模板，从模板一键创建 Runtime 配置，降低重复配置成本。

## 需求范围

- [ ] 内置模板：`Spring Boot Development`（JDK 21 / Profile dev / -Xms512m -Xmx2048m / DevTools enabled）（§83）
- [ ] 模板模型：与 R-07 配置同构的子集 + 模板元信息（名称 / 描述 / 适用类型）
- [ ] 自定义模板 CRUD：从现有 Runtime 配置「另存为模板」、编辑、删除
- [ ] 模板存储：`.gitworkspace/templates/*.json`（可 Git 版本化、团队共享，同 R-07 约定）
- [ ] 从模板创建 Runtime 配置：向导中模板选择 → 预填 → 覆盖项修改
- [ ] UI：模板列表 + 应用入口（创建应用向导内）

## 架构 / 性能注意点

- 模板只做「创建时预填」，不与已创建配置保持联动（避免模板改动级联影响存量配置）。
- 内置模板随版本升级可更新，但不得覆盖用户同名自定义模板。
- 模板 JSON 校验与 R-07 配置加载同一套规则。

## 验收标准

- [x] 从内置模板一键创建配置并成功启动样例应用（载荷预填 + R-07 create_config 全量校验，启动链路复用 R-09/R-10 既有测试覆盖）
- [x] 「另存为模板 → 复用创建」闭环可用（`save_as_template_strips_identity_and_reapplies`）
- [x] 模板文件可团队共享（`.gitworkspace/templates/` 放仓库即生效，与 R-07 同约定）
- [x] 内置模板升级不覆盖用户自定义（`user_template_shadows_builtin_and_survives_upgrade`：同名用户文件遮蔽内置）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：模板模型与存储 + 内置模板 + CRUD + 向导接入 |
| 2026-08-29 | ✅ | 完成：templates.rs（模型/存储/内置/遮蔽语义）+ 5 个 IPC 命令 + 向导「从模板创建 / 另存为模板」。测试 `cargo test --lib runtime::` 168 通过（含同名遮蔽、路径穿越拒绝、身份剥离复用），golden 快照同步，vue-tsc 通过 |

### 子任务清单

- [x] 模板模型与存储（`.gitworkspace/templates/<name>.json`，与 R-07 同构子集 + 元信息）
- [x] 内置模板（Spring Boot Development：JDK 21 / dev / -Xms512m -Xmx2048m / DevTools 开）
- [x] 模板 CRUD + 另存为（身份字段剥离；builtin 标记 IPC 不可伪造）
- [x] 创建向导接入（模板选择预填 → 修改 → 经 runtime_apply_template 落盘）
- [x] 单元测试
