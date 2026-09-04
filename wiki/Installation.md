# 安装与下载

## 下载安装包

CI 会把预构建安装包发布到 [Releases](https://github.com/mantougg/git-workspace/releases) 页面：

| 平台 | 产物 |
| --- | --- |
| Windows | NSIS 安装程序（`GitWorkspace_x.x.x_x64-setup.exe`） |
| macOS | .app / .dmg bundle |
| Linux | bundle |

### Windows SmartScreen「未知发布者」提示

Windows 首次运行安装包时可能弹出 SmartScreen 警告。这是因为二进制**没有购买 Authenticode 代码签名证书**（OV/EV 证书需付费向 CA 购买），不代表文件有问题。选择「仍要运行」即可。

注意区分两种签名：

- **Tauri updater 签名**（`.sig` 文件）：用于应用内自动更新时校验安装包完整性，与 SmartScreen 无关；
- **Windows Authenticode 签名**：消除 SmartScreen 的唯一途径，配置方式见 [构建与发版](Build-and-Release)。

## 系统要求

- Windows 10/11（需 WebView2，一般系统自带）、macOS、Linux
- 日常使用**不需要**预装 JDK / Maven / Node——没有工具链时 Runtime 功能会给出可行动的提示
- 网络操作（fetch / pull / push）走系统 `git` CLI，以复用系统凭据管理器与 SSH 配置，建议安装 [Git](https://git-scm.com/)

## 从源码构建

环境要求：Node.js ≥ 18（推荐 20+）与 pnpm、Rust stable 工具链、Git CLI；Linux 另需 `webkit2gtk-4.1` 等 Tauri 2 系统依赖。

```bash
pnpm install          # 安装前端依赖
pnpm tauri dev        # 开发模式（首次编译 Rust 依赖耗时较长）
pnpm tauri build      # 打包（Windows 为 NSIS 安装包）
```

更多开发者内容见 [构建与发版](Build-and-Release)。

## 数据存储

所有数据保存在本机系统应用数据目录下的 `gitworkspace.db`（SQLite，WAL 模式）：

- Windows：`%APPDATA%\com.gitworkspace.app`
- macOS：`~/Library/Application Support/com.gitworkspace.app`
- Linux：`~/.config/com.gitworkspace.app`

卸载应用**不会**自动删除该目录；如需彻底清理请手动删除。
