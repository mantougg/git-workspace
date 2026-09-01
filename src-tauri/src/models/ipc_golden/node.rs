//! Node.js IPC samples (N-02 / N-08).

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::commands::node::NodeInstallRequest;
use crate::node::model::{NodeExecutable, NodeExecutableKind, NodeExecutableRequest, PackageManager};
use crate::node::NodeProjectNode;

pub(super) fn samples(m: &mut Map<String, Value>) {
    m.insert(
        "NodeProjectNode".into(),
        json!(NodeProjectNode {
            project_id: 7,
            repository_id: Some(3),
            path: PathBuf::from("/ws/web"),
            name: "web".into(),
            version: "1.2.3".into(),
            package_manager: Some("npm".into()),
            scripts_json: r#"{"dev":"vite","build":"vite build"}"#.into(),
            pkg_hash: "0123456789abcdef".into(),
        }),
    );
    m.insert(
        "NodeExecutable".into(),
        json!(NodeExecutable {
            id: Some(1),
            kind: NodeExecutableKind::PackageManager,
            package_manager: Some(PackageManager::Pnpm),
            executable_path: "/usr/local/bin/pnpm".into(),
            version: Some("11.24.0".into()),
            raw_output: "11.24.0".into(),
            is_valid: true,
            last_checked: "2026-09-01T00:00:00+00:00".into(),
            created_at: Some("2026-09-01T00:00:00+00:00".into()),
            updated_at: Some("2026-09-01T00:00:00+00:00".into()),
        }),
    );
    m.insert(
        "NodeExecutableRequest".into(),
        json!(NodeExecutableRequest {
            kind: NodeExecutableKind::PackageManager,
            package_manager: Some(PackageManager::Pnpm),
            executable_path: "/usr/local/bin/pnpm".into(),
        }),
    );
    m.insert(
        "NodeInstallRequest".into(),
        json!(NodeInstallRequest {
            project_dir: "/ws/web".into(),
            package_manager: PackageManager::Pnpm,
            confirmed: false,
        }),
    );
}

pub(super) const TS_TYPE_MAP: &[(&str, &str, &str)] = &[
    ("NodeProjectNode", "types/node.ts", "NodeProjectNode"),
    ("NodeExecutable", "types/node.ts", "NodeExecutable"),
    ("NodeExecutableRequest", "types/node.ts", "NodeExecutableRequest"),
    ("NodeInstallRequest", "types/node.ts", "NodeInstallRequest"),
];
