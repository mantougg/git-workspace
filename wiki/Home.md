# GitWorkspace Wiki

> 面向「一个项目拆成几十个 Git 仓库」的桌面开发工作台 —— 跨仓库批量 Git 操作、不启动 IDE 构建/运行 Spring Boot 与 Node.js 服务、自带 Key 的 AI 辅助审查。免费（MIT）、离线优先，基于 Tauri 2 + Vue 3 + Rust。

这里是 GitWorkspace 的使用手册。想找「项目是干嘛的、30 秒上手」请回 [README](https://github.com/mantougg/git-workspace)；想找任务拆分与技术方案等开发过程文档，见主仓库的 [`docs/`](https://github.com/mantougg/git-workspace/tree/master/docs) 目录。

## 页面导航

- **[安装与下载](Installation)** —— 安装包、系统要求、SmartScreen 提示说明、源码构建
- **[工作区与 Git 操作](Workspace-and-Git)** —— 多仓库工作区、变更树与批量操作、分支 / Stash / Rebase / 冲突解决
- **[Runtime 工作台](Runtime)** —— 不打开 IDE 构建/运行 Spring Boot 与 Node.js 服务、日志、健康探针、端口管理
- **[工具箱](Toolbox)** —— 26 个内置开发小工具：编解码、速查表、生成器、网络工具
- **[AI 助手](AI-Assistant)** —— 自带 Key 接入 OpenAI / Anthropic / 兼容网关，隐私与脱敏设计
- **[常见问题](FAQ)** —— 批量操作原理、数据存储位置、离线使用、报错排查
- **[构建与发版](Build-and-Release)** —— 开发者向：构建、性能基准、发布流水线与签名

## 一分钟速览

1. [Releases](https://github.com/mantougg/git-workspace/releases) 下载安装包（Windows NSIS / macOS / Linux）;
2. 打开应用，把项目根目录添加为**工作区**——其中嵌套的几十个仓库会被自动发现；
3. 首页「变更」查看整个工作区的改动树，勾选后批量 Add / Commit / Push;
4. 需要跑服务时进 **Runtime** 页，选择应用直接启动，日志与端口状态实时可见；
5. 侧边栏「工具箱」提供端口查询、JSON 格式化、速查表等日常小工具。
