//! Build Engine（R-09，§28 Build 流程、§29 Start 抽象、§30 Run Strategy、§73）。
//!
//! 把 Spring Boot 的 Maven 构建步骤串成可观测流水线，并抽象 Maven 与
//! Node.js 的启动策略：
//!
//! - [`RunStrategy::MavenRun`]：`mvn spring-boot:run`；
//! - [`RunStrategy::PackageRun`]：`mvn package` + `java -jar`；
//! - [`RunStrategy::ClasspathRun`]：`mvn compile` + `dependency:build-classpath`
//!   + `java -cp`；
//! - [`RunStrategy::NodeScript`]：直接执行包管理器 `run <script>`。
//!
//! 边界（任务文档）：构建范围只含 Runtime Closure 模块；Java 编译缓存完全
//! 依赖 Maven 原生 `~/.m2`（§73 第一阶段）；并发由 [`scheduler::BuildScheduler`]
//! 限流（全局约束 §6，默认最大并发 Build = 2）。本任务只做后端引擎与测试，
//! IPC 命令与前端在 R-12 / R-13 接入。
//!
//! 模块内 `LaunchPlan` / 构建环境含未脱敏秘密，**不 derive Serialize**，不得
//! 直接暴露到 IPC（全局约束 §4）。

pub mod classpath;
pub mod dep_cache;
pub mod node_engine;
pub mod pathing_jar;
pub mod pipeline;
pub mod runner;
pub mod scheduler;
pub mod strategy;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::maven::exec_model::MavenExecutionRequest;
use crate::maven::reactor::RuntimeReactorKind;
use crate::process::streaming::OutputStream;

/// Run Strategy（§30）。`camelCase` 序列化：mavenRun / packageRun /
/// classpathRun / nodeScript。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunStrategy {
    /// `mvn spring-boot:run`：Maven 进程内直接启动。
    MavenRun,
    /// `mvn package` + `java -jar`：先出产物再独立启动。
    PackageRun,
    /// `mvn compile` + 解析 classpath + `java -cp`：跳过打包，启动最快。
    ClasspathRun,
    /// 直接执行 Node.js package.json script。
    NodeScript,
}

impl RunStrategy {
    /// 稳定字符串（与 serde 一致，便于日志与配置存取）。
    pub fn as_str(self) -> &'static str {
        match self {
            RunStrategy::MavenRun => "mavenRun",
            RunStrategy::PackageRun => "packageRun",
            RunStrategy::ClasspathRun => "classpathRun",
            RunStrategy::NodeScript => "nodeScript",
        }
    }

    /// 从字符串还原；未识别值返回 `None`，由调用方转成可行动错误。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mavenRun" => Some(RunStrategy::MavenRun),
            "packageRun" => Some(RunStrategy::PackageRun),
            "classpathRun" => Some(RunStrategy::ClasspathRun),
            "nodeScript" => Some(RunStrategy::NodeScript),
            _ => None,
        }
    }
}

/// 默认策略（§30）：Production-like profile（prod / production，大小写不敏感）
/// → Package Run；Development 及未设置 → Classpath Run。
/// 最终默认以 R-08 Benchmark 数据校准（任务文档「需求范围」）。
pub fn default_strategy(profile: Option<&str>) -> RunStrategy {
    match profile.map(|p| p.to_ascii_lowercase()) {
        Some(p) if p == "prod" || p == "production" => RunStrategy::PackageRun,
        _ => RunStrategy::ClasspathRun,
    }
}

/// 一次 Build 的请求。
#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub workspace_id: i64,
    /// Runtime 配置名（`.gitworkspace/runtimes/<name>.json`）。
    pub runtime_name: String,
    pub options: BuildOptions,
}

/// Build 选项。默认值对齐 IDEA 的 Build 语义：不跑测试、在线、30 分钟超时。
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// 显式指定策略；`None` 时按 [`default_strategy`] 由 profile 推断。
    pub strategy: Option<RunStrategy>,
    /// 跳过测试（仅 `package` 阶段注入 `-DskipTests`）。默认 true。
    pub skip_tests: bool,
    /// Maven `-o` 离线模式。
    pub offline: bool,
    /// 透传给 Maven 构建调用的额外参数。
    pub extra_maven_args: Vec<String>,
    /// 构建超时；`None` 表示不限。默认 Some(30min)。
    pub timeout: Option<Duration>,
    /// R-10 Launcher 注入：R-06 检测推断出的 mainClass（配置 `mainClass`
    /// 缺省时）；`Some` 时在加载配置后覆盖生效，不改动用户配置文件。
    pub main_class_override: Option<String>,
    /// R-18 §73 第二阶段：Runtime Dependency Cache（模块输入指纹未变则
    /// 跳过重建）。默认开启；指纹设计「宁可重建不错过」。
    pub dependency_cache: bool,
    /// R-17 §44：watch 影响分析给出的必建模块 GA 子集（非空时与指纹子集
    /// 合并，且阻止 SkipAll——显式变更信号优先于「指纹未变」判断）。
    pub affected_modules: Vec<String>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            strategy: None,
            skip_tests: true,
            offline: false,
            extra_maven_args: Vec::new(),
            timeout: Some(Duration::from_secs(30 * 60)),
            main_class_override: None,
            dependency_cache: true,
            affected_modules: Vec::new(),
        }
    }
}

/// 启动计划（§29 Start 抽象）：Build 完成后交给 R-10 Launcher 的启动描述。
///
/// **内部类型，不 derive Serialize**——`env` 含未脱敏秘密，禁止跨 IPC。
#[derive(Debug, Clone)]
pub enum LaunchPlan {
    /// 以 Maven goal 启动（`spring-boot:run`）。
    MavenGoal {
        request: MavenExecutionRequest,
        env: Vec<(String, String)>,
        /// 完整命令预览（§75 可预览/可追溯）。
        preview: String,
    },
    /// `java -jar <artifact>.jar`。
    JavaJar {
        java_exec: PathBuf,
        jar_path: PathBuf,
        vm_options: Vec<String>,
        program_arguments: Vec<String>,
        env: Vec<(String, String)>,
        working_dir: PathBuf,
        preview: String,
    },
    /// `java -cp <classes + deps> <main-class>`。
    JavaClasspath {
        java_exec: PathBuf,
        /// 首元素是应用模块自身的 `target/classes`，其余为依赖 jar。
        classpath: Vec<PathBuf>,
        main_class: String,
        vm_options: Vec<String>,
        program_arguments: Vec<String>,
        env: Vec<(String, String)>,
        working_dir: PathBuf,
        preview: String,
    },
    /// 以包管理器执行 npm script（`npm run dev` / `pnpm run dev`）。
    Script {
        executable: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
        working_dir: PathBuf,
        preview: String,
    },
}

/// Build 成功结果。
#[derive(Debug)]
pub struct BuildOutcome {
    pub strategy: RunStrategy,
    pub reactor_kind: RuntimeReactorKind,
    pub reactor_pom: PathBuf,
    /// 本次构建覆盖的模块（`groupId:artifactId`，依赖序）。
    pub modules_built: Vec<String>,
    pub build_duration_ms: u128,
    /// 实际执行的 Maven 构建命令预览（§75）。
    pub build_command_preview: String,
    pub launch: LaunchPlan,
}

/// 构建输出的消费端（R-11 日志引擎 / R-12 进度事件的挂接点）。
pub trait BuildOutputSink {
    fn on_line(&mut self, stream: OutputStream, line: &str);
}

/// `BuildFailed.log_tail` 的尾部环形缓冲：引擎内部始终维护，保留最后
/// ~200 行且总量不超过 32KB。只记已脱敏的行（脱敏在 pipeline 层先做）。
pub struct RingTail {
    lines: VecDeque<String>,
    bytes: usize,
}

const RING_TAIL_MAX_LINES: usize = 200;
const RING_TAIL_MAX_BYTES: usize = 32 * 1024;

impl RingTail {
    pub fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            bytes: 0,
        }
    }

    /// 当前尾部内容（`\n` 连接）。
    pub fn tail(&self) -> String {
        self.lines.iter().map(String::as_str).collect::<Vec<_>>().join("\n")
    }
}

impl Default for RingTail {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildOutputSink for RingTail {
    fn on_line(&mut self, _stream: OutputStream, line: &str) {
        self.bytes += line.len() + 1;
        self.lines.push_back(line.to_string());
        while self.lines.len() > RING_TAIL_MAX_LINES || self.bytes > RING_TAIL_MAX_BYTES {
            if let Some(evicted) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(evicted.len() + 1);
            } else {
                self.bytes = 0;
                break;
            }
        }
    }
}

/// 引擎执行上下文：把流水线需要的共享设施打包，供 [`BuildEngine`] 使用。
///
/// `db` 是共享连接（Arc<Mutex>）：流水线按阶段短持锁（R-12），Maven 子
/// 进程运行期间不占用 SQLite 写锁。
pub struct BuildContext<'a> {
    pub db: &'a std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    pub workspace_root: &'a std::path::Path,
    pub graph_cache: &'a crate::maven::DependencyGraphCache,
    pub closure_cache: &'a crate::maven::RuntimeClosureCache,
    pub scheduler: &'a scheduler::BuildScheduler,
    pub runner: &'a dyn runner::MavenRunner,
    /// R-14 §75：Pre/Post Build Script 确认状态。
    pub script_approvals: &'a crate::runtime::script_approval::ScriptApprovalStore,
}

/// Build Engine 抽象（§29）。Maven 先行实现；mvnd（R-18）/ Gradle（R-22）
/// 经 [`engine_for`] 的 id 分发预留接入位，本任务不提前实现。
pub trait BuildEngine {
    fn id(&self) -> &'static str;
    fn build(
        &self,
        cx: &mut BuildContext<'_>,
        request: &BuildRequest,
        sink: &mut dyn BuildOutputSink,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> AppResult<BuildOutcome>;
}

/// 按 id 分发 Build Engine；未知 id → `RuntimeConfig` 可行动错误。
/// `mvnd`（R-18）与 `maven` 共用 Maven 构建流水线——差异只在可执行体解析
/// （[`runner::BuildEngineHint`]）与 daemon 回退，Engine 抽象不变。
pub fn engine_for(id: &str) -> AppResult<Box<dyn BuildEngine>> {
    match id {
        "node" => Ok(Box::new(node_engine::NodeBuildEngine)),
        "maven" | "mvnd" => Ok(Box::new(pipeline::MavenBuildEngine)),
        other => Err(AppError::RuntimeConfig(format!(
            "未知的 Build Engine '{other}'；当前支持 'maven' / 'mvnd' / 'node'（Gradle 由 R-22 预留）"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_strategy_roundtrips_stable_strings() {
        for (strategy, text) in [
            (RunStrategy::MavenRun, "mavenRun"),
            (RunStrategy::PackageRun, "packageRun"),
            (RunStrategy::ClasspathRun, "classpathRun"),
            (RunStrategy::NodeScript, "nodeScript"),
        ] {
            assert_eq!(strategy.as_str(), text);
            assert_eq!(RunStrategy::parse(text), Some(strategy));
            assert_eq!(serde_json::to_value(strategy).unwrap(), serde_json::json!(text));
        }
        assert_eq!(RunStrategy::parse("unknown"), None);
    }

    #[test]
    fn default_strategy_maps_production_like_profiles_to_package_run() {
        assert_eq!(default_strategy(None), RunStrategy::ClasspathRun);
        assert_eq!(default_strategy(Some("dev")), RunStrategy::ClasspathRun);
        assert_eq!(default_strategy(Some("prod")), RunStrategy::PackageRun);
        assert_eq!(default_strategy(Some("Production")), RunStrategy::PackageRun);
        assert_eq!(default_strategy(Some("PROD")), RunStrategy::PackageRun);
    }

    #[test]
    fn build_options_defaults_match_idea_build_semantics() {
        let options = BuildOptions::default();
        assert!(options.skip_tests);
        assert!(!options.offline);
        assert!(options.strategy.is_none());
        assert_eq!(options.timeout, Some(Duration::from_secs(30 * 60)));
    }

    #[test]
    fn ring_tail_keeps_last_lines_within_byte_cap() {
        let mut tail = RingTail::new();
        for i in 0..250 {
            tail.on_line(OutputStream::Stdout, &format!("line-{i}"));
        }
        let content = tail.tail();
        assert!(content.contains("line-249"));
        assert!(!content.contains("line-0"));
        assert!(content.len() <= RING_TAIL_MAX_BYTES);
        assert!(tail.lines.len() <= RING_TAIL_MAX_LINES);
    }

    #[test]
    fn engine_for_rejects_unknown_ids_actionably() {
        assert_eq!(engine_for("maven").unwrap().id(), "maven");
        // R-18：mvnd 与 maven 共用流水线，是合法 engine id。
        assert_eq!(engine_for("mvnd").unwrap().id(), "maven");
        assert_eq!(engine_for("node").unwrap().id(), "node");
        let error = engine_for("gradle").err().expect("unknown engine must fail");
        assert_eq!(error.code(), "RuntimeConfigError");
        assert!(error.to_string().contains("gradle"));
    }
}
