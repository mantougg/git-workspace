//! Node.js IPC samples (N-02).

use std::path::PathBuf;

use serde_json::{json, Map, Value};

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
}

pub(super) const TS_TYPE_MAP: &[(&str, &str, &str)] =
    &[("NodeProjectNode", "types/node.ts", "NodeProjectNode")];
