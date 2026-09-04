# 构建与发版（开发者向）

> 使用手册之外的内容。任务拆分、技术方案等开发过程文档在主仓库 [`docs/`](https://github.com/mantougg/git-workspace/tree/master/docs)。

## 环境要求

- [Node.js](https://nodejs.org/) ≥ 18（推荐 20+）与 [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/) stable 工具链（含 `cargo`)
- [Git](https://git-scm.com/) CLI
- Tauri 2 系统依赖：Windows 需 WebView2(Win10/11 一般自带）、macOS 需 Xcode CLT、Linux 需 `webkit2gtk-4.1` 等

## 常用命令

```bash
pnpm install          # 安装前端依赖
pnpm tauri dev        # 开发模式（首次编译 Rust 依赖耗时较长，之后增量）
pnpm dev              # 仅前端 Vite dev server（浏览器调试 UI，端口 1420）
pnpm build            # vue-tsc 类型检查 + 前端构建（输出 dist/）
pnpm tauri build      # 打包桌面应用（Windows 为 NSIS 安装包）
```

## 性能基准

CI 有 Benchmark 门禁（`.github/workflows/benchmark.yml`)，每次推送强制校验：100 仓库首扫 < 2s、单仓库状态刷新 < 100ms、Diff 缓存命中 < 50ms、提交图首屏 < 1s。本地运行：

```bash
cargo run --release --example benchmark -- 100          # 100 个仓库扫描基准
cargo run --release --example benchmark -- diff-graph   # Diff / 提交图基准
```

## 发布流水线

由 `.github/workflows/release.yml` 自动构建三平台安装包并发布到 Releases:

| 触发 | 结果 |
| --- | --- |
| 推版本 tag(`git tag v0.4.0 && git push origin v0.4.0`) | 正式发布，版本号取自 tag |
| Actions 页手动 Run workflow | Draft Release，版本号 `0.0.0-dev.<run#>` |

工作流会把 tag 版本同步进 `tauri.conf.json` 再构建，产物版本号不会重复。

## 签名（两层，别混淆）

1. **Tauri updater 签名**（可选，应用内自动更新用）：本地生成一次密钥对
   `pnpm tauri signer generate -w ~/.tauri/gitworkspace.key`，把私钥配进仓库 Secrets(`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`)，公钥填进 `tauri.conf.json` 的 `plugins.updater.pubkey`。私钥丢失 = 已安装的旧版本收不到更新。
2. **Windows Authenticode 代码签名**（消除 SmartScreen)：需向 CA 购买 OV/EV 证书，在 `tauri.conf.json` 的 `bundle.windows` 配 `certificateThumbprint` / `digestAlgorithm` / `timestampUrl`;CI 中把 `.pfx` 以 base64 存 Secret 后导入再构建。详见 [Tauri 官方文档](https://v2.tauri.app/distribute/sign/windows/)。

## Wiki 维护

本 Wiki 的源文件在主仓库的 [`wiki/`](https://github.com/mantougg/git-workspace/tree/master/wiki) 目录——改文档走正常 PR 流程，push 到 master 后由 `.github/workflows/sync-wiki.yml` 自动同步到 Wiki。
