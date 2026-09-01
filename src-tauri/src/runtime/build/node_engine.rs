//! Node.js BuildEngine adapter (N-04).
//!
//! Node frontend projects do not need Maven's dependency graph or reactor.
//! The actual direct path lives beside the Maven pipeline so it can reuse the
//! same environment, script-confirmation, error and output handling.

use std::sync::atomic::AtomicBool;

use crate::error::AppResult;
use crate::runtime::build::pipeline::execute_node_build;
use crate::runtime::build::{
    BuildContext, BuildEngine, BuildOutcome, BuildOutputSink, BuildRequest,
};

pub struct NodeBuildEngine;

impl BuildEngine for NodeBuildEngine {
    fn id(&self) -> &'static str {
        "node"
    }

    fn build(
        &self,
        cx: &mut BuildContext<'_>,
        request: &BuildRequest,
        sink: &mut dyn BuildOutputSink,
        cancel: Option<&AtomicBool>,
    ) -> AppResult<BuildOutcome> {
        execute_node_build(
            cx.db,
            cx.workspace_root,
            cx.script_approvals,
            request,
            sink,
            cancel,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::maven::closure::RuntimeClosureCache;
    use crate::maven::index::DependencyGraphCache;
    use crate::node::{detect_node, detect_package_manager, PackageManager};
    use crate::process::streaming::{spawn_streaming, OutputStream};
    use crate::runtime::build::runner::fake::FakeMavenRunner;
    use crate::runtime::build::scheduler::BuildScheduler;
    use crate::runtime::build::{BuildOutputSink, BuildRequest, LaunchPlan, RunStrategy};
    use crate::runtime::config::{
        create_config, CreateRuntimeConfigRequest, RuntimeApplicationConfig, RuntimeKind,
    };
    use crate::runtime::script_approval::ScriptApprovalStore;

    struct Sink(Vec<String>);

    impl BuildOutputSink for Sink {
        fn on_line(&mut self, _stream: OutputStream, line: &str) {
            self.0.push(line.to_string());
        }
    }

    fn fixture() -> (PathBuf, Arc<Mutex<rusqlite::Connection>>, i64) {
        let root = std::env::temp_dir().join(format!("gw_n04_node_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("web/node_modules")).unwrap();
        std::fs::write(
            root.join("web/package.json"),
            r#"{"name":"web","scripts":{"dev":"node -e \"console.log('node-fixture')\""}}"#,
        )
        .unwrap();
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        (root, Arc::new(Mutex::new(conn)), workspace_id)
    }

    #[test]
    fn node_engine_returns_script_plan_without_maven_calls() {
        if detect_node().is_err() || detect_package_manager(PackageManager::Npm).is_err() {
            eprintln!("N-04: node/npm unavailable; skipping NodeBuildEngine test");
            return;
        }
        let (root, db, workspace_id) = fixture();
        let project = root.join("web");
        let mut config = RuntimeApplicationConfig {
            name: "web".into(),
            project: project.to_string_lossy().into_owned(),
            kind: RuntimeKind::Node,
            node_script: Some("dev".into()),
            ..Default::default()
        };
        config.node_package_manager = Some("npm".into());
        create_config(
            &db.lock().unwrap(),
            &CreateRuntimeConfigRequest {
                workspace_id,
                config,
            },
        )
        .unwrap();
        let request = BuildRequest {
            workspace_id,
            runtime_name: "web".into(),
            options: Default::default(),
        };
        let approvals = ScriptApprovalStore::new(root.join("approvals.json"));
        let mut sink = Sink(Vec::new());
        let graph_cache = DependencyGraphCache::new();
        let closure_cache = RuntimeClosureCache::new();
        let scheduler = BuildScheduler::new(1);
        let runner = FakeMavenRunner::successful();
        let outcome = crate::runtime::build::pipeline::execute_build(
            &db,
            &root,
            &graph_cache,
            &closure_cache,
            &scheduler,
            &runner,
            &request,
            &approvals,
            &mut sink,
            None,
        )
        .unwrap();
        assert_eq!(runner.request_count(), 0, "Node path must not call Maven");
        assert_eq!(outcome.strategy, RunStrategy::NodeScript);
        assert!(outcome.build_command_preview.contains("npm"));
        let LaunchPlan::Script {
            executable,
            args,
            working_dir,
            ..
        } = outcome.launch
        else {
            panic!("Node engine must return LaunchPlan::Script");
        };
        assert_eq!(args, ["run", "dev"]);
        assert_eq!(working_dir, project);
        let mut command = crate::runtime::launch::launcher::launch_command(
            &LaunchPlan::Script {
                executable,
                args,
                env: vec![],
                working_dir: working_dir.clone(),
                preview: String::new(),
            },
            1,
            "web",
        )
        .unwrap();
        let mut output = Vec::new();
        let exit = spawn_streaming(
            &mut command,
            None,
            Some(Duration::from_secs(20)),
            &mut |_stream, line| output.push(line.to_string()),
        )
        .unwrap();
        assert_eq!(exit.exit_code, Some(0));
        assert!(output.iter().any(|line| line.contains("node-fixture")));
        let _ = std::fs::remove_dir_all(root);
    }

    /// N-08 验收：pnpm / yarn 样例工程真实启动闭环。参数形状差异由
    /// `build_run_args` 决定（两者均无 `--` 分隔），启动复用同一 spawn 链。
    fn node_engine_launch_loopback(manager: PackageManager, manager_name: &str, marker: &str) {
        if detect_node().is_err() || detect_package_manager(manager).is_err() {
            eprintln!(
                "N-08: node/{} unavailable; skipping real launch loopback",
                manager_name
            );
            return;
        }
        let (root, db, workspace_id) = fixture();
        let project = root.join("web");
        std::fs::write(
            project.join("package.json"),
            format!(
                r#"{{"name":"web","scripts":{{"dev":"node -e \"console.log('{}')\""}}}}"#,
                marker
            ),
        )
        .unwrap();
        create_config(
            &db.lock().unwrap(),
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "web".into(),
                    project: project.to_string_lossy().into_owned(),
                    kind: RuntimeKind::Node,
                    node_script: Some("dev".into()),
                    node_package_manager: Some(manager_name.into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        let request = BuildRequest {
            workspace_id,
            runtime_name: "web".into(),
            options: Default::default(),
        };
        let mut sink = Sink(Vec::new());
        let outcome = crate::runtime::build::pipeline::execute_build(
            &db,
            &root,
            &DependencyGraphCache::new(),
            &RuntimeClosureCache::new(),
            &BuildScheduler::new(1),
            &FakeMavenRunner::successful(),
            &request,
            &ScriptApprovalStore::new(root.join("approvals.json")),
            &mut sink,
            None,
        )
        .unwrap();
        assert_eq!(outcome.strategy, RunStrategy::NodeScript);
        assert!(
            outcome.build_command_preview.contains(manager_name),
            "preview should mention {}: {}",
            manager_name,
            outcome.build_command_preview
        );
        let LaunchPlan::Script {
            executable,
            args,
            working_dir,
            ..
        } = outcome.launch
        else {
            panic!("{} engine must return LaunchPlan::Script", manager_name);
        };
        assert_eq!(args, ["run", "dev"]);
        assert_eq!(working_dir, project);
        let mut command = crate::runtime::launch::launcher::launch_command(
            &LaunchPlan::Script {
                executable,
                args,
                env: vec![],
                working_dir: working_dir.clone(),
                preview: String::new(),
            },
            1,
            "web",
        )
        .unwrap();
        let mut output = Vec::new();
        let exit = spawn_streaming(
            &mut command,
            None,
            Some(Duration::from_secs(30)),
            &mut |_stream, line| output.push(line.to_string()),
        )
        .unwrap();
        assert_eq!(exit.exit_code, Some(0));
        assert!(
            output.iter().any(|line| line.contains(marker)),
            "{} run dev output should contain {}: {:?}",
            manager_name,
            marker,
            output
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pnpm_engine_launches_real_dev_script() {
        node_engine_launch_loopback(PackageManager::Pnpm, "pnpm", "pnpm-fixture");
    }

    #[test]
    fn yarn_engine_launches_real_dev_script() {
        node_engine_launch_loopback(PackageManager::Yarn, "yarn", "yarn-fixture");
    }

    #[test]
    fn node_engine_reports_missing_dependencies_without_installing() {
        if detect_node().is_err() || detect_package_manager(PackageManager::Npm).is_err() {
            eprintln!("N-04: node/npm unavailable; skipping missing node_modules test");
            return;
        }
        let (root, db, workspace_id) = fixture();
        let project = root.join("web");
        std::fs::remove_dir_all(project.join("node_modules")).unwrap();
        create_config(
            &db.lock().unwrap(),
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "web".into(),
                    project: project.to_string_lossy().into_owned(),
                    kind: RuntimeKind::Node,
                    node_script: Some("dev".into()),
                    node_package_manager: Some("npm".into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        let request = BuildRequest {
            workspace_id,
            runtime_name: "web".into(),
            options: Default::default(),
        };
        let mut sink = Sink(Vec::new());
        let error = crate::runtime::build::pipeline::execute_node_build(
            &db,
            &root,
            &ScriptApprovalStore::new(root.join("approvals.json")),
            &request,
            &mut sink,
            None,
        )
        .unwrap_err();
        assert_eq!(error.code(), "RuntimeConfigError");
        assert!(error.to_string().contains("node_install"));
        let _ = std::fs::remove_dir_all(root);
    }
}
