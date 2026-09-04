# AI 助手

AI 功能**完全可选、默认关闭**——应用可离线运行，不依赖任何外部服务。

## 接入方式（自带 Key）

在「AI 设置」页配置 Provider、模型与凭据，支持三种协议：

- **OpenAI Chat Completions**——OpenAI、DeepSeek 及任意兼容网关；
- **OpenAI Responses**;
- **Anthropic Messages**——Claude 系列及兼容网关。

## 功能

- **AI 代码审查**：对工作区 diff 输出结构化问题列表（严重级别 / 类别 / 文件）;
- **AI 提交信息**：根据暂存改动生成提交信息；
- **AI 冲突解决**：辅助合并冲突；
- **AI PR 描述与安全审查**：生成 PR 描述，附缺陷检测与提交解读；
- **助手抽屉**：以只读 Git 与 Runtime 工具（状态、Diff、日志、本地 FTS5 代码检索）对话，并给出**行动提案**——所有写操作执行前先预览确认。

## 隐私设计

- API Key 存入**操作系统钥匙串**（Windows Credential Manager / macOS Keychain / Secret Service），永不写入明文文件；
- 提示词发出前**自动脱敏**（密钥、密码等敏感值不会发给模型）;
- 扫描、状态、Diff、检索、构建、运行全部本地完成——不用 AI 时，代码不离开本机。
