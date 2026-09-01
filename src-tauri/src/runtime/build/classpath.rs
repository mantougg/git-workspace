//! Classpath 解析与按模块缓存（R-09，§73 Build Cache Strategy）。
//!
//! 对应用根模块执行 `dependency:build-classpath`（runtime scope），产物写入
//! `<workspace>/.gitworkspace/runtime/<runtime>/classpath/<artifactId>-<hash>.txt`。
//! `hash = hex(pom_hash + graph_fingerprint + local_repository)`：POM、依赖图
//! 或本地仓库配置任一变化都会得到新 hash，未变则直接复用、跳过 Maven 调用。
//! 同 artifactId 的旧 hash 文件在写入新产物前清理。
//!
//! 缓存内容 = 依赖 jar 列表；模块自身的 `target/classes` 不在这里，由
//! Classpath Run 策略在拼 LaunchPlan 时放在首元素。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::maven::exec_model::{MavenExecutable, MavenExecutionRequest};
use crate::maven::executor;
use crate::maven::index::MavenProjectNode;
use crate::maven::reactor::RuntimeReactorPlan;

/// `dependency:build-classpath` 只取运行时 scope（§30 Classpath Run）。
const CLASSPATH_INCLUDE_SCOPE: &str = "runtime";

/// Classpath 缓存目录：`<workspace>/.gitworkspace/runtime/<runtime>/classpath/`。
pub fn classpath_cache_dir(workspace_root: &Path, runtime_name: &str) -> PathBuf {
    workspace_root
        .join(".gitworkspace")
        .join("runtime")
        .join(runtime_name)
        .join("classpath")
}

/// 缓存 key：根模块 pom_hash + 依赖图指纹 + 本地仓库路径。
pub fn classpath_cache_key(
    root: &MavenProjectNode,
    graph_fingerprint: &str,
    local_repository: &Path,
) -> String {
    let material = format!(
        "{}\0{}\0{}",
        root.pom_hash,
        graph_fingerprint,
        local_repository.to_string_lossy()
    );
    crate::maven::parser::hex_hash(material.as_bytes())
}

/// 缓存文件路径：`<dir>/<artifactId>-<hash>.txt`。
pub fn classpath_cache_file(dir: &Path, artifact_id: &str, key: &str) -> PathBuf {
    dir.join(format!("{artifact_id}-{key}.txt"))
}

/// 命中即返回依赖 jar 列表；未命中返回 `None`（由调用方触发 Maven 生成）。
pub fn cached_classpath(
    workspace_root: &Path,
    runtime_name: &str,
    root: &MavenProjectNode,
    graph_fingerprint: &str,
    local_repository: &Path,
) -> Option<Vec<PathBuf>> {
    let dir = classpath_cache_dir(workspace_root, runtime_name);
    let key = classpath_cache_key(root, graph_fingerprint, local_repository);
    let file = classpath_cache_file(&dir, &root.coordinates.artifact_id, &key);
    read_classpath_file(&file).ok()
}

/// 准备一次缓存写入：建目录、清理同 artifactId 的旧 hash 文件，返回目标路径。
pub fn prepare_cache_write(dir: &Path, artifact_id: &str, key: &str) -> AppResult<PathBuf> {
    fs::create_dir_all(dir)?;
    let target = classpath_cache_file(dir, artifact_id, key);
    let prefix = format!("{artifact_id}-");
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".txt") && entry.path() != target {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(target)
}

/// 读取 `dependency:build-classpath` 产物文件（平台路径分隔符连接）。
pub fn read_classpath_file(path: &Path) -> AppResult<Vec<PathBuf>> {
    let content = fs::read_to_string(path).map_err(|error| {
        AppError::Io(std::io::Error::new(
            error.kind(),
            format!("无法读取 classpath 缓存 {}：{error}", path.display()),
        ))
    })?;
    Ok(std::env::split_paths(content.trim()).collect())
}

/// 构造 `dependency:build-classpath` 的 Maven 请求。
///
/// 复用 Reactor plan 的 `-f` 参数；goals 为 `process-classes dependency:build-classpath`
/// 并在多模块时加 `-pl <app g:a> -am`：
/// - 单独跑 `build-classpath` 时，Reactor 内库模块的 jar 尚未打包，Maven 会改去
///   本地/远程仓库找（未 install 时直接失败）；先跑 `process-classes` 让库模块的
///   输出目录进入会话，Reactor 解析才能落到 `target/classes`（增量编译是 no-op，
///   代价可忽略）；
/// - `-am` 把闭包内的库模块留在会话里（Reactor 拓扑序保证应用模块最后执行，
///   共享的 `outputFile` 最终内容即应用模块的 classpath）。
pub fn build_classpath_request(
    executable: &MavenExecutable,
    workspace_root: &Path,
    reactor: &RuntimeReactorPlan,
    root: &MavenProjectNode,
    output_file: &Path,
    offline: bool,
    local_repository: Option<PathBuf>,
) -> MavenExecutionRequest {
    let mut extra_args = vec!["-f".into(), reactor.pom_path.to_string_lossy().into_owned()];
    if reactor.module_paths.len() > 1 {
        extra_args.extend([
            "-pl".into(),
            format!(
                "{}:{}",
                root.coordinates.group_id, root.coordinates.artifact_id
            ),
            "-am".into(),
        ]);
    }
    extra_args.push(format!(
        "-Dmdep.outputFile={}",
        output_file.to_string_lossy()
    ));
    extra_args.push(format!("-Dmdep.includeScope={CLASSPATH_INCLUDE_SCOPE}"));
    if offline {
        extra_args.push("-o".into());
    }
    executor::build_request(
        executable,
        workspace_root,
        vec![
            "process-classes".into(),
            "dependency:build-classpath".into(),
        ],
        extra_args,
        local_repository,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::model::PomCoordinates;
    use crate::maven::reactor::RuntimeReactorKind;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_r09_cp_{name}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn node(hash: &str) -> MavenProjectNode {
        MavenProjectNode {
            project_id: 1,
            repository_id: Some(1),
            path: PathBuf::from("/ws/repo/app/pom.xml"),
            coordinates: PomCoordinates {
                group_id: "com.example".into(),
                artifact_id: "app".into(),
                version: "1.0.0".into(),
            },
            packaging: "jar".into(),
            pom_hash: hash.into(),
        }
    }

    #[test]
    fn cache_write_read_hit_and_hash_change() {
        let root_dir = temp_dir("hit");
        let runtime = "app";
        let local_repo = root_dir.join("m2");

        // 未生成前未命中。
        assert!(cached_classpath(&root_dir, runtime, &node("h1"), "fp1", &local_repo).is_none());

        let dir = classpath_cache_dir(&root_dir, runtime);
        let key = classpath_cache_key(&node("h1"), "fp1", &local_repo);
        let target = prepare_cache_write(&dir, "app", &key).unwrap();
        let entries = vec![root_dir.join("m2/a.jar"), root_dir.join("m2/b.jar")];
        fs::write(
            &target,
            std::env::join_paths(&entries)
                .unwrap()
                .to_string_lossy()
                .as_ref(),
        )
        .unwrap();

        // 同输入命中。
        let hit = cached_classpath(&root_dir, runtime, &node("h1"), "fp1", &local_repo).unwrap();
        assert_eq!(hit, entries);

        // pom_hash 或图指纹变化 → 新 key → 未命中。
        assert!(cached_classpath(&root_dir, runtime, &node("h2"), "fp1", &local_repo).is_none());
        assert!(cached_classpath(&root_dir, runtime, &node("h1"), "fp2", &local_repo).is_none());

        // 写入新 hash 时清理旧文件。
        let key2 = classpath_cache_key(&node("h2"), "fp1", &local_repo);
        let target2 = prepare_cache_write(&dir, "app", &key2).unwrap();
        fs::write(&target2, "").unwrap();
        assert!(!target.exists(), "stale hash file must be removed");
        assert!(target2.exists());
        let _ = fs::remove_dir_all(root_dir);
    }

    #[test]
    fn classpath_request_carries_reactor_output_file_and_scope() {
        let reactor = RuntimeReactorPlan {
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
        };
        let exe = MavenExecutable::new(
            "/usr/bin/mvn",
            crate::maven::exec_model::MavenSource::System,
            None,
        );
        let request = build_classpath_request(
            &exe,
            Path::new("/ws"),
            &reactor,
            &node("h"),
            Path::new("/tmp/cp.txt"),
            true,
            Some(PathBuf::from("/m2")),
        );
        let command = executor::build_command(&request);
        assert!(command.contains(&"dependency:build-classpath".to_string()));
        assert!(command.contains(&"-Dmdep.outputFile=/tmp/cp.txt".to_string()));
        assert!(command.contains(&"-Dmdep.includeScope=runtime".to_string()));
        assert!(command.contains(&"-o".to_string()));
        assert!(command.contains(&"-Dmaven.repo.local=/m2".to_string()));
        // 复用 reactor 的 -f，带 -pl 与 -am（库模块须留在 Reactor 会话里供解析）。
        assert!(command.contains(&"-pl".to_string()));
        assert!(command.contains(&"com.example:app".to_string()));
        assert!(command.contains(&"-am".to_string()));
    }

    #[test]
    fn single_module_reactor_omits_project_list() {
        let reactor = RuntimeReactorPlan {
            kind: RuntimeReactorKind::Existing,
            pom_path: PathBuf::from("/ws/repo/app/pom.xml"),
            module_paths: vec![PathBuf::from("/ws/repo/app")],
            arguments: vec!["-f".into(), "/ws/repo/app/pom.xml".into()],
        };
        let exe = MavenExecutable::new(
            "/usr/bin/mvn",
            crate::maven::exec_model::MavenSource::System,
            None,
        );
        let request = build_classpath_request(
            &exe,
            Path::new("/ws"),
            &reactor,
            &node("h"),
            Path::new("/tmp/cp.txt"),
            false,
            None,
        );
        assert!(!request.extra_args.contains(&"-pl".to_string()));
        assert!(!request.extra_args.contains(&"-o".to_string()));
    }
}
