//! 三种 Run Strategy 的构建命令与 LaunchPlan 构造（R-09，§30）。
//!
//! - build goals：MavenRun / ClasspathRun → `compile`；PackageRun → `package`
//!   （`skip_tests` 时仅 package 注入 `-DskipTests`，对齐 IDEA Build 语义）。
//! - Maven 调用的 `extra_args` 直接整组携带 [`RuntimeReactorPlan::arguments`]
//!   （已含 `-f pom [-pl g:a -am]`），随后追加策略 flags、`offline` 与
//!   用户 `extra_maven_args`。
//! - LaunchPlan 只做「启动所需的一切」的纯数据构造；实际启动由 R-10
//!   Launcher 完成。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::java::model::JdkInstallation;
use crate::maven::exec_model::{MavenExecutable, MavenExecutionRequest};
use crate::maven::executor;
use crate::maven::index::MavenProjectNode;
use crate::maven::reactor::RuntimeReactorPlan;
use crate::runtime::build::{BuildOptions, LaunchPlan, RunStrategy};
use crate::runtime::config::RuntimeApplicationConfig;

/// 各策略的构建 goals。
pub fn build_goals(strategy: RunStrategy) -> Vec<String> {
    match strategy {
        RunStrategy::MavenRun | RunStrategy::ClasspathRun => vec!["compile".into()],
        RunStrategy::PackageRun => vec!["package".into()],
    }
}

/// 构造 Maven 构建请求：reactor 参数 + 策略 flags + offline + 用户额外参数。
pub fn build_maven_request(
    executable: &MavenExecutable,
    workspace_root: &Path,
    reactor: &RuntimeReactorPlan,
    strategy: RunStrategy,
    options: &BuildOptions,
    local_repository: Option<PathBuf>,
) -> MavenExecutionRequest {
    let mut extra_args = reactor.arguments.clone();
    if strategy == RunStrategy::PackageRun && options.skip_tests {
        extra_args.push("-DskipTests".into());
    }
    if options.offline {
        extra_args.push("-o".into());
    }
    extra_args.extend(options.extra_maven_args.iter().cloned());
    executor::build_request(
        executable,
        workspace_root,
        build_goals(strategy),
        extra_args,
        local_repository,
    )
}

/// LaunchPlan 构造的输入集合。
pub struct LaunchInputs<'a> {
    pub config: &'a RuntimeApplicationConfig,
    pub root: &'a MavenProjectNode,
    pub reactor: &'a RuntimeReactorPlan,
    pub executable: &'a MavenExecutable,
    pub workspace_root: &'a Path,
    pub local_repository: Option<PathBuf>,
    /// 构建环境（五层合并 + JAVA_HOME），未脱敏。
    pub env: Vec<(String, String)>,
    pub jdk: Option<&'a JdkInstallation>,
    /// Classpath Run 的依赖 jar 列表（不含模块自身 target/classes）。
    pub classpath: Option<Vec<PathBuf>>,
}

/// 按策略构造启动计划。
pub fn launch_plan(strategy: RunStrategy, inputs: &LaunchInputs) -> AppResult<LaunchPlan> {
    match strategy {
        RunStrategy::MavenRun => maven_run_plan(inputs),
        RunStrategy::PackageRun => package_run_plan(inputs),
        RunStrategy::ClasspathRun => classpath_run_plan(inputs),
    }
}

/// `groupId:artifactId` 模块坐标。
pub fn module_ga(node: &MavenProjectNode) -> String {
    format!(
        "{}:{}",
        node.coordinates.group_id, node.coordinates.artifact_id
    )
}

/// 模块目录（`node.path` 是 pom 文件路径）。
pub fn module_directory(node: &MavenProjectNode) -> PathBuf {
    node.path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf()
}

/// 所选 JDK 的 `java` 可执行路径；无配置 JDK 时回退 PATH 中的 `java`。
pub fn java_exec_for(jdk: Option<&JdkInstallation>) -> PathBuf {
    if let Some(jdk) = jdk {
        if let Some(exec) = &jdk.java_exec {
            return PathBuf::from(exec);
        }
        let bin = Path::new(&jdk.home_path).join("bin");
        return bin.join(if cfg!(windows) { "java.exe" } else { "java" });
    }
    PathBuf::from("java")
}

/// MavenRun：`spring-boot:run` 只在应用模块上执行——带 `-f <reactor pom>`
/// 与（多模块时）`-pl <app g:a>`，**不带 `-am`**，避免在库模块上跑 run 目标。
fn maven_run_plan(inputs: &LaunchInputs) -> AppResult<LaunchPlan> {
    let mut extra_args = vec![
        "-f".into(),
        inputs.reactor.pom_path.to_string_lossy().into_owned(),
    ];
    if inputs.reactor.module_paths.len() > 1 {
        extra_args.extend(["-pl".into(), module_ga(inputs.root)]);
    }
    let request = executor::build_request(
        inputs.executable,
        inputs.workspace_root,
        vec!["spring-boot:run".into()],
        extra_args,
        inputs.local_repository.clone(),
    );
    let preview = executor::preview_command(&request);
    Ok(LaunchPlan::MavenGoal {
        request,
        env: inputs.env.clone(),
        preview,
    })
}

/// PackageRun：`java -jar <module>/target/<artifactId>-<version>.jar`。
fn package_run_plan(inputs: &LaunchInputs) -> AppResult<LaunchPlan> {
    let config = inputs.config.with_default_profile_injection();
    let module_dir = module_directory(inputs.root);
    let jar_path = module_dir.join("target").join(format!(
        "{}-{}.jar",
        inputs.root.coordinates.artifact_id, inputs.root.coordinates.version
    ));
    if !jar_path.is_file() {
        return Err(AppError::BuildFailed {
            module: module_ga(inputs.root),
            exit_code: None,
            log_tail: format!(
                "Maven package 完成但未找到产物 jar：{}。\
                 请确认模块 packaging 为 jar 且 spring-boot-maven-plugin 的 repackage 配置正确",
                jar_path.display()
            ),
        });
    }
    let java_exec = java_exec_for(inputs.jdk);
    let preview = java_preview(
        &java_exec,
        &config.vm_options,
        &[format!("-jar {}", jar_path.display())],
        &config.program_arguments,
    );
    Ok(LaunchPlan::JavaJar {
        java_exec,
        jar_path,
        vm_options: config.vm_options,
        program_arguments: config.program_arguments,
        env: inputs.env.clone(),
        working_dir: module_dir,
        preview,
    })
}

/// ClasspathRun：`java -cp <target/classes + 依赖 jars> <main-class>`。
fn classpath_run_plan(inputs: &LaunchInputs) -> AppResult<LaunchPlan> {
    let config = inputs.config.with_default_profile_injection();
    let main_class = config.main_class.clone().ok_or_else(|| {
        AppError::RuntimeConfig(format!(
            "Classpath Run 需要 mainClass：请在 Runtime 配置 '{}' 中设置 mainClass\
             （R-10/R-13 将接入 R-06 的自动推断）",
            config.name
        ))
    })?;
    let module_dir = module_directory(inputs.root);
    let mut classpath = vec![module_dir.join("target").join("classes")];
    classpath.extend(inputs.classpath.clone().unwrap_or_default());
    let java_exec = java_exec_for(inputs.jdk);
    let cp = std::env::join_paths(&classpath)
        .map(|cp| cp.to_string_lossy().into_owned())
        .unwrap_or_default();
    let preview = java_preview(
        &java_exec,
        &config.vm_options,
        &["-cp".into(), cp, main_class.clone()],
        &config.program_arguments,
    );
    Ok(LaunchPlan::JavaClasspath {
        java_exec,
        classpath,
        main_class,
        vm_options: config.vm_options,
        program_arguments: config.program_arguments,
        env: inputs.env.clone(),
        working_dir: module_dir,
        preview,
    })
}

fn java_preview(
    java_exec: &Path,
    vm_options: &[String],
    app_args: &[String],
    program_arguments: &[String],
) -> String {
    let mut parts = vec![java_exec.to_string_lossy().into_owned()];
    parts.extend(vm_options.iter().cloned());
    parts.extend(app_args.iter().cloned());
    parts.extend(program_arguments.iter().cloned());
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::exec_model::MavenSource;
    use crate::maven::model::PomCoordinates;
    use crate::maven::reactor::RuntimeReactorKind;
    use std::fs;

    fn node(id: i64, artifact: &str, path: &str) -> MavenProjectNode {
        MavenProjectNode {
            project_id: id,
            repository_id: Some(1),
            path: PathBuf::from(path),
            coordinates: PomCoordinates {
                group_id: "com.example".into(),
                artifact_id: artifact.into(),
                version: "1.0.0".into(),
            },
            packaging: "jar".into(),
            pom_hash: format!("hash-{id}"),
        }
    }

    fn executable() -> MavenExecutable {
        MavenExecutable::new("/usr/bin/mvn", MavenSource::System, None)
    }

    fn multi_module_reactor() -> RuntimeReactorPlan {
        RuntimeReactorPlan {
            kind: RuntimeReactorKind::Existing,
            pom_path: PathBuf::from("/ws/repo/pom.xml"),
            module_paths: vec![PathBuf::from("/ws/repo/lib"), PathBuf::from("/ws/repo/app")],
            arguments: vec![
                "-f".into(),
                "/ws/repo/pom.xml".into(),
                "-pl".into(),
                "com.example:app".into(),
                "-am".into(),
            ],
        }
    }

    fn single_module_reactor() -> RuntimeReactorPlan {
        RuntimeReactorPlan {
            kind: RuntimeReactorKind::Existing,
            pom_path: PathBuf::from("/ws/repo/app/pom.xml"),
            module_paths: vec![PathBuf::from("/ws/repo/app")],
            arguments: vec!["-f".into(), "/ws/repo/app/pom.xml".into()],
        }
    }

    fn options() -> BuildOptions {
        BuildOptions::default()
    }

    #[test]
    fn package_build_adds_skip_tests_compile_does_not() {
        let reactor = multi_module_reactor();
        let app = node(2, "app", "/ws/repo/app/pom.xml");
        let _ = app;
        let package = build_maven_request(
            &executable(),
            Path::new("/ws"),
            &reactor,
            RunStrategy::PackageRun,
            &options(),
            None,
        );
        assert_eq!(package.goals, ["package"]);
        assert!(package.extra_args.contains(&"-DskipTests".to_string()));
        // reactor 参数整组透传。
        assert_eq!(
            package.extra_args[..5],
            ["-f", "/ws/repo/pom.xml", "-pl", "com.example:app", "-am"]
        );

        let compile = build_maven_request(
            &executable(),
            Path::new("/ws"),
            &reactor,
            RunStrategy::ClasspathRun,
            &options(),
            None,
        );
        assert_eq!(compile.goals, ["compile"]);
        assert!(!compile.extra_args.contains(&"-DskipTests".to_string()));
    }

    #[test]
    fn offline_and_extra_args_are_appended() {
        let reactor = single_module_reactor();
        let mut opts = options();
        opts.offline = true;
        opts.extra_maven_args = vec!["-Pprod".into()];
        let request = build_maven_request(
            &executable(),
            Path::new("/ws"),
            &reactor,
            RunStrategy::MavenRun,
            &opts,
            Some(PathBuf::from("/m2")),
        );
        assert!(request.extra_args.contains(&"-o".to_string()));
        assert!(request.extra_args.contains(&"-Pprod".to_string()));
        let command = executor::build_command(&request);
        assert!(command.contains(&"-Dmaven.repo.local=/m2".to_string()));
    }

    #[test]
    fn maven_run_never_carries_am_and_scopes_to_app_module() {
        let config = RuntimeApplicationConfig {
            name: "app".into(),
            project: "app".into(),
            ..Default::default()
        };
        let app = node(2, "app", "/ws/repo/app/pom.xml");
        let inputs = LaunchInputs {
            config: &config,
            root: &app,
            reactor: &multi_module_reactor(),
            executable: &executable(),
            workspace_root: Path::new("/ws"),
            local_repository: None,
            env: vec![],
            jdk: None,
            classpath: None,
        };
        let plan = launch_plan(RunStrategy::MavenRun, &inputs).unwrap();
        let LaunchPlan::MavenGoal { request, preview, .. } = plan else {
            panic!("expected MavenGoal");
        };
        assert_eq!(request.goals, ["spring-boot:run"]);
        assert!(request.extra_args.contains(&"-pl".to_string()));
        assert!(request.extra_args.contains(&"com.example:app".to_string()));
        assert!(!request.extra_args.contains(&"-am".to_string()));
        assert!(preview.contains("spring-boot:run"));

        // 单项目直通：只有 -f，没有 -pl。
        let single = LaunchInputs {
            reactor: &single_module_reactor(),
            ..inputs
        };
        let plan = launch_plan(RunStrategy::MavenRun, &single).unwrap();
        let LaunchPlan::MavenGoal { request, .. } = plan else {
            panic!("expected MavenGoal");
        };
        assert!(!request.extra_args.contains(&"-pl".to_string()));
    }

    #[test]
    fn package_run_locates_jar_and_injects_profile() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_r09_strategy_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let module_dir = tmp.join("app");
        fs::create_dir_all(module_dir.join("target")).unwrap();
        let jar = module_dir.join("target/app-1.0.0.jar");
        fs::write(&jar, b"jar").unwrap();
        fs::write(module_dir.join("pom.xml"), b"<project/>").unwrap();

        let config = RuntimeApplicationConfig {
            name: "app".into(),
            project: "app".into(),
            profile: Some("prod".into()),
            program_arguments: vec!["--server.port=9090".into()],
            ..Default::default()
        };
        let app = node(
            2,
            "app",
            module_dir.join("pom.xml").to_string_lossy().as_ref(),
        );
        let inputs = LaunchInputs {
            config: &config,
            root: &app,
            reactor: &single_module_reactor(),
            executable: &executable(),
            workspace_root: &tmp,
            local_repository: None,
            env: vec![],
            jdk: None,
            classpath: None,
        };
        let plan = launch_plan(RunStrategy::PackageRun, &inputs).unwrap();
        let LaunchPlan::JavaJar {
            jar_path,
            vm_options,
            program_arguments,
            preview,
            ..
        } = plan
        else {
            panic!("expected JavaJar");
        };
        assert_eq!(jar_path, jar);
        assert!(vm_options.contains(&"-Dspring.profiles.active=prod".to_string()));
        assert_eq!(program_arguments, ["--server.port=9090"]);
        assert!(preview.contains("-jar"));
        assert!(preview.contains("-Dspring.profiles.active=prod"));

        // 缺失 jar → BuildFailed。
        fs::remove_file(module_dir.join("target/app-1.0.0.jar")).unwrap();
        let error = launch_plan(RunStrategy::PackageRun, &inputs).unwrap_err();
        assert_eq!(error.code(), "BuildFailed");
        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn classpath_run_requires_main_class_and_puts_classes_first() {
        let tmp = PathBuf::from("/ws");
        let app = node(2, "app", "/ws/repo/app/pom.xml");
        let config = RuntimeApplicationConfig {
            name: "app".into(),
            project: "app".into(),
            main_class: Some("com.example.Application".into()),
            ..Default::default()
        };
        let inputs = LaunchInputs {
            config: &config,
            root: &app,
            reactor: &single_module_reactor(),
            executable: &executable(),
            workspace_root: &tmp,
            local_repository: None,
            env: vec![],
            jdk: None,
            classpath: Some(vec![PathBuf::from("/m2/spring.jar")]),
        };
        let plan = launch_plan(RunStrategy::ClasspathRun, &inputs).unwrap();
        let LaunchPlan::JavaClasspath {
            classpath,
            main_class,
            ..
        } = plan
        else {
            panic!("expected JavaClasspath");
        };
        assert_eq!(main_class, "com.example.Application");
        assert_eq!(classpath[0], PathBuf::from("/ws/repo/app/target/classes"));
        assert!(classpath.contains(&PathBuf::from("/m2/spring.jar")));

        // 未配置 mainClass → 可行动 RuntimeConfig 错误。
        let no_main = RuntimeApplicationConfig {
            main_class: None,
            ..config.clone()
        };
        let error = launch_plan(
            RunStrategy::ClasspathRun,
            &LaunchInputs {
                config: &no_main,
                ..inputs
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "RuntimeConfigError");
        assert!(error.to_string().contains("mainClass"));
    }
}
