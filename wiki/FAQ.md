# 常见问题（FAQ）

## 使用

**怎么在几十个仓库上批量 pull / fetch / push？**
把项目根目录添加为工作区，勾选仓库（或全选），执行一次即可。所有操作走后台任务队列：实时进度、可取消，并记录在可撤销的操作日志里。

**能不能不打开 IntelliJ / VS Code 就运行 Spring Boot / Node.js 服务？**
可以——这正是 [Runtime 工作台](Runtime)。GitWorkspace 检测你的 JDK / Maven / Node 工具链，推断每个应用的构建与启动方式，流式输出日志、运行健康探针、管理端口。

**批量操作做错了能撤回吗？**
能。每个批量操作都有统一操作日志，支持 Undo 兜底（`git reset` / `git stash` 类不可逆操作除外，执行前会有明确确认提示）。

**数据存在哪里？卸载会丢吗？**
见 [安装与下载](Installation) 的「数据存储」。SQLite 在系统应用数据目录，卸载应用不会删除它。

## 安全与隐私

**我的代码会离开本机吗？**
不会。扫描、状态、Diff、检索、构建、运行全部本地完成。AI 功能严格可选：使用你自己的 API Key（存系统钥匙串），请求发出前自动脱敏。

**收费吗？**
MIT 协议，免费，无需注册账号。

## 故障排查

**Windows 安装时 SmartScreen 拦了一下？**
二进制未购买 Authenticode 签名证书，属预期提示，选「仍要运行」。详见 [安装与下载](Installation)。

**Runtime 提示找不到 JDK / Maven / Node？**
应用不会硬失败。到对应工具链管理页（JDK 管理 / Maven 设置 / Node 工具链）扫描或手动添加即可；真实构建 Spring Boot 3.x 需要 JDK 17+。

**端口被占用，服务起不来？**
工具箱 → 端口查询，输入端口看占用进程（PID / 路径），确认后可安全终止；或在 Runtime 配置里改用别的端口。

**git fetch / pull 报认证错误？**
网络操作走系统 `git` CLI，凭据取自系统配置（Windows Credential Manager / SSH agent）。先在终端里手动 `git fetch` 一次确认系统凭据可用。
