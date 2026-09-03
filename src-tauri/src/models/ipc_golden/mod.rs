//! IPC type single-source-of-truth tests (T-03, global constraint §6).
//!
//! Rust serde structs are the source of truth for every payload crossing IPC
//! (command args/returns and Tauri event payloads). Two tests guard against
//! drift between the Rust definitions and the hand-written TS types:
//!
//! 1. `golden_samples_match_snapshot` — serializes a representative sample of
//!    every IPC type and compares it against `golden/ipc_samples.json`.
//!    Regenerate after intentional changes with
//!    `GW_UPDATE_GOLDEN=1 cargo test ipc_golden` and review the git diff.
//! 2. `ts_types_match_rust_samples` — parses the TS type files and asserts
//!    each TS type's field set matches the keys of its Rust sample (and, for
//!    tagged enums, each union variant). Every exported type in the mapped
//!    files must be registered in `TS_TYPE_MAP`.
//!
//! ts-rs codegen remains a later evaluation item; until then this is the
//! automated backstop against `camelCase` renames and field add/remove drift.
//!
//! Split by domain (B-01): `runtime` / `git` / `task` / `common` siblings hold
//! their sample sections and TS_TYPE_MAP slices; this module merges them and
//! hosts the two snapshot/consistency tests and TS parsing helpers.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use std::path::PathBuf;

use serde_json::{Map, Value};

mod common;
mod git;
mod node;
mod runtime;
mod task;

/// Representative sample of every IPC type, keyed by Rust type name.
/// Enum (tagged-union) types serialize as an array of all variants.
/// Domain sections live in the sibling modules and are merged here.
fn samples() -> Map<String, Value> {
    let mut m = Map::new();

    common::samples(&mut m);
    runtime::samples(&mut m);
    git::samples(&mut m);
    node::samples(&mut m);
    task::samples(&mut m);

    m
}

/// (golden key, TS file relative to the frontend `src/` dir, TS type name).
/// Every payload crossing IPC must be registered here (domain slices merged).
fn ts_type_map() -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    common::TS_TYPE_MAP
        .iter()
        .chain(runtime::TS_TYPE_MAP.iter())
        .chain(git::TS_TYPE_MAP.iter())
        .chain(node::TS_TYPE_MAP.iter())
        .chain(task::TS_TYPE_MAP.iter())
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/ipc_samples.json")
}

fn frontend_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have a parent dir")
        .join("src")
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Snapshot test: serialized samples must match the committed golden file.
/// Regenerate with `GW_UPDATE_GOLDEN=1 cargo test ipc_golden`.
#[test]
fn golden_samples_match_snapshot() {
    let path = golden_path();
    let actual = serde_json::to_string_pretty(&Value::Object(samples())).unwrap() + "\n";

    if std::env::var("GW_UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "golden file missing at {}; create it with `GW_UPDATE_GOLDEN=1 cargo test ipc_golden`",
            path.display()
        )
    });
    assert_eq!(
        normalize(&expected),
        normalize(&actual),
        "IPC serialization drift vs golden/ipc_samples.json; if intentional, \
         regenerate with `GW_UPDATE_GOLDEN=1 cargo test ipc_golden` and review the git diff"
    );
}

#[derive(Default)]
struct TsFileTypes {
    /// interface name -> field names
    interfaces: HashMap<String, BTreeSet<String>>,
    /// tagged-union name -> (tag value -> field names, including `type`)
    unions: HashMap<String, BTreeMap<String, BTreeSet<String>>>,
}

/// Remove `/* ... */` blocks and `//` line comments so comment text cannot
/// produce false field matches. Not string-literal aware; sufficient for the
/// type-declaration files parsed here.
fn strip_ts_comments(content: &str) -> String {
    let block = regex::Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let line = regex::Regex::new(r"//[^\n]*").unwrap();
    line.replace_all(&block.replace_all(content, ""), "").to_string()
}

/// Parse the exported interfaces and tagged-union type aliases of a TS file.
/// Only handles the simple declaration shapes used under `src/` (one field
/// per line; union variants as object literals).
fn parse_ts_file(content: &str) -> TsFileTypes {
    let interface_start = regex::Regex::new(r"^export interface (\w+)\s*\{").unwrap();
    let union_start = regex::Regex::new(r"^export type (\w+)\s*=").unwrap();
    // Interface fields: one per line, anchored.
    let iface_field_re = regex::Regex::new(r"^\s*([A-Za-z_]\w*)\??\s*:").unwrap();
    // Union variant fields: multiple per line, unanchored.
    let variant_field_re = regex::Regex::new(r"([A-Za-z_]\w*)\??\s*:").unwrap();
    // Discriminant field: `type`, `status`, or domain-specific `mode`.
    let tag_re = regex::Regex::new(r#"(?:type|status|mode)\s*:\s*"([^"]+)""#).unwrap();

    let mut result = TsFileTypes::default();
    let stripped = strip_ts_comments(content);
    let mut lines = stripped.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();

        if let Some(cap) = interface_start.captures(trimmed) {
            let name = cap[1].to_string();
            let mut fields = BTreeSet::new();
            for body in lines.by_ref() {
                let b = body.trim();
                if b == "}" {
                    break;
                }
                if let Some(f) = iface_field_re.captures(b) {
                    fields.insert(f[1].to_string());
                }
            }
            result.interfaces.insert(name, fields);
            continue;
        }

        if let Some(cap) = union_start.captures(trimmed) {
            let name = cap[1].to_string();
            // Accumulate the union body until the line that terminates the
            // type alias: braces balanced and line ends with `;` (multi-line
            // variants have `;`-terminated field lines inside the braces).
            let mut body = String::new();
            let mut rest = trimmed[cap.get(0).unwrap().end()..].to_string();
            let mut depth = 0i32;
            loop {
                for ch in rest.chars() {
                    match ch {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                let done = depth <= 0 && rest.trim_end().ends_with(';');
                body.push_str(&rest);
                body.push('\n');
                if done {
                    break;
                }
                match lines.next() {
                    Some(next) => rest = next.to_string(),
                    None => break,
                }
            }
            // Pure string-literal unions (e.g. `FailurePolicy`, `CloneAction`)
            // carry no `{...}` object variants, so the parser cannot validate
            // them as tagged unions; skip them so the reverse coverage check
            // does not demand their registration.
            if !body.contains('{') {
                continue;
            }
            // Each top-level `{ ... }` group is one variant.
            let mut variants = BTreeMap::new();
            let mut depth = 0i32;
            let mut current = String::new();
            for ch in body.chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        if depth == 1 {
                            current.clear();
                            continue;
                        }
                    }
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            let tag = tag_re.captures(&current).map(|c| c[1].to_string()).unwrap_or_default();
                            let fields: BTreeSet<String> = variant_field_re
                                .captures_iter(&current)
                                .map(|c| c[1].to_string())
                                .collect();
                            variants.insert(tag, fields);
                            continue;
                        }
                    }
                    _ => {}
                }
                if depth >= 1 {
                    current.push(ch);
                }
            }
            result.unions.insert(name, variants);
        }
    }

    result
}

fn rust_keys(sample: &Value) -> BTreeSet<String> {
    sample
        .as_object()
        .expect("struct sample must be a JSON object")
        .keys()
        .cloned()
        .collect()
}

/// Discriminant of a tagged-union sample variant.
fn variant_tag(v: &Value) -> String {
    v["type"]
        .as_str()
        .or_else(|| v["status"].as_str())
        .or_else(|| v["mode"].as_str())
        .expect("tagged-union variant must carry `type`, `status`, or `mode`")
        .to_string()
}

/// Alignment test: TS field sets must match the Rust sample keys exactly.
#[test]
fn ts_types_match_rust_samples() {
    let samples = Value::Object(samples());
    let root = frontend_src_dir();

    // Parse each mapped TS file once.
    let mut parsed: HashMap<&str, TsFileTypes> = HashMap::new();
    for (_, file, _) in ts_type_map() {
        if parsed.contains_key(file) {
            continue;
        }
        let path = root.join(file);
        let content =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
        parsed.insert(file, parse_ts_file(&content));
    }

    for (golden_key, file, ts_name) in ts_type_map() {
        let sample = samples
            .get(golden_key)
            .unwrap_or_else(|| panic!("no Rust sample registered for `{}`", golden_key));
        let types = &parsed[file];

        if let Value::Array(variants) = sample {
            // Tagged union: every Rust variant must have a TS counterpart
            // with the same tag and field set, and vice versa.
            let union = types.unions.get(*ts_name).unwrap_or_else(|| {
                panic!(
                    "TS union type `{}` not found in src/{} — drift or rename",
                    ts_name, file
                )
            });
            let rust_tags: BTreeSet<String> = variants.iter().map(variant_tag).collect();
            let ts_tags: BTreeSet<String> = union.keys().cloned().collect();
            assert_eq!(rust_tags, ts_tags, "`{}` variant tags drifted (src/{})", ts_name, file);
            for v in variants {
                let tag = variant_tag(v);
                assert_eq!(
                    rust_keys(v),
                    union[&tag],
                    "`{}` variant `{}` fields drifted (src/{})",
                    ts_name,
                    tag,
                    file
                );
            }
        } else {
            let fields = types
                .interfaces
                .get(*ts_name)
                .unwrap_or_else(|| panic!("TS interface `{}` not found in src/{} — drift or rename", ts_name, file));
            assert_eq!(
                &rust_keys(sample),
                fields,
                "`{}` (src/{}) fields drifted from Rust `{}`",
                ts_name,
                file,
                golden_key
            );
        }
    }

    // Reverse coverage: every exported type in the mapped files must be
    // registered in TS_TYPE_MAP, so new TS-only types cannot slip through.
    for (file, types) in &parsed {
        let mapped: BTreeSet<&str> = ts_type_map()
            .filter(|(_, f, _)| f == file)
            .map(|(_, _, ts)| *ts)
            .collect();
        for name in types.interfaces.keys().chain(types.unions.keys()) {
            assert!(
                mapped.contains(name.as_str()),
                "TS type `{}` in src/{} is not registered in ts_type_map() (models/ipc_golden/)",
                name,
                file
            );
        }
    }
}
