//! 单文件符号 / 引用提取（T-28，纯函数）。
//!
//! 查询捕获 `@name` / `@def` 得到符号；kind 与容器（所属 impl / class /
//! 接口 / Go receiver）由定义节点推断（`classify`），引用走 `@ref` /
//! `@call` 捕获并按 (name, line) 合并（任一调用形态即 is_call=1）。

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

use super::lang::LangConfig;

pub struct SymbolRec {
    pub name: String,
    pub kind: &'static str,
    pub line: usize,
    pub end_line: usize,
    pub container: Option<String>,
    pub signature: Option<String>,
}

pub struct RefRec {
    pub name: String,
    pub line: usize,
    pub is_call: bool,
}

#[derive(Default)]
pub struct FileExtraction {
    pub symbols: Vec<SymbolRec>,
    pub refs: Vec<RefRec>,
}

/// 解析并提取单文件。解析失败（未产树）返回 None；语法错误尽量容错
///（tree-sitter 错误恢复节点照样可查）。
pub fn extract(lang: &LangConfig, code: &str) -> Option<FileExtraction> {
    let mut parser = Parser::new();
    parser.set_language(&lang.language).ok()?;
    let tree = parser.parse(code, None)?;
    Some(extract_from_tree(lang, &tree, code))
}

fn extract_from_tree(lang: &LangConfig, tree: &Tree, code: &str) -> FileExtraction {
    let src = code.as_bytes();
    let mut out = FileExtraction::default();

    // --- 符号 ---
    let sym_query = match Query::new(&lang.language, lang.symbol_query) {
        Ok(q) => q,
        Err(_) => return out, // lang 测试保证可编译；此处防御
    };
    let mut cursor = QueryCursor::new();
    let mut seen_defs: HashSet<usize> = HashSet::new();
    let mut def_name_nodes: HashSet<usize> = HashSet::new();

    let mut matches = cursor.matches(&sym_query, tree.root_node(), src);
    while let Some(m) = matches.next() {
        let mut name: Option<(String, usize)> = None;
        let mut def: Option<Node> = None;
        for cap in m.captures {
            let idx = cap.index as usize;
            let node = cap.node;
            // 按 query 中捕获名解析（symbol_query 只用 name/def 两个名字）。
            match sym_query.capture_names()[idx] {
                "name" => {
                    name = Some((node.utf8_text(src).unwrap_or("").to_string(), node.id()));
                }
                "def" => def = Some(node),
                _ => {}
            }
        }
        let (def_node, (name_text, name_id)) = match (def, name) {
            (Some(d), Some(n)) => (d, n),
            _ => continue,
        };
        if !seen_defs.insert(def_node.id()) {
            continue;
        }
        if !valid_symbol_name(&name_text) {
            continue;
        }
        def_name_nodes.insert(name_id);

        let (kind, container) = classify(lang.id, def_node, src);
        let line = def_node.start_position().row + 1;
        let end_line = def_node.end_position().row + 1;
        let signature = signature_of(def_node, src);
        out.symbols.push(SymbolRec {
            name: name_text,
            kind,
            line,
            end_line,
            container,
            signature,
        });
    }

    // --- 引用与调用 ---
    let ref_query = match Query::new(&lang.language, lang.ref_query) {
        Ok(q) => q,
        Err(_) => return out,
    };
    let call_query = match Query::new(&lang.language, lang.call_query) {
        Ok(q) => q,
        Err(_) => return out,
    };

    // 调用名节点集合：与引用标识符同一节点，用于打 is_call。
    let mut call_nodes: HashSet<usize> = HashSet::new();
    let mut calls = cursor.matches(&call_query, tree.root_node(), src);
    while let Some(m) = calls.next() {
        for cap in m.captures {
            if call_query.capture_names()[cap.index as usize] == "call" {
                call_nodes.insert(cap.node.id());
            }
        }
    }

    // (name, line) → is_call 合并
    let mut merged: HashMap<(String, usize), bool> = HashMap::new();
    let mut refs = cursor.matches(&ref_query, tree.root_node(), src);
    while let Some(m) = refs.next() {
        for cap in m.captures {
            if ref_query.capture_names()[cap.index as usize] != "ref" {
                continue;
            }
            let node = cap.node;
            if def_name_nodes.contains(&node.id()) {
                continue; // 定义点本身不算引用
            }
            let name = match node.utf8_text(src) {
                Ok(n) if valid_symbol_name(n) => n.to_string(),
                _ => continue,
            };
            let line = node.start_position().row + 1;
            let is_call = call_nodes.contains(&node.id());
            merged
                .entry((name, line))
                .and_modify(|c| *c |= is_call)
                .or_insert(is_call);
        }
    }
    out.refs = merged
        .into_iter()
        .map(|((name, line), is_call)| RefRec {
            name,
            line,
            is_call,
        })
        .collect();
    out.refs
        .sort_by(|a, b| a.line.cmp(&b.line).then(a.name.cmp(&b.name)));
    out
}

fn valid_symbol_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// 定义节点的签名（首行截断 120 字符）。
fn signature_of(def_node: Node, src: &[u8]) -> Option<String> {
    let text = def_node.utf8_text(src).ok()?;
    let first = text.lines().next()?.trim();
    let mut sig = first.to_string();
    if text.lines().count() > 1 && !sig.ends_with('{') {
        // 跨行签名补省略号
        sig.push_str(" …");
    }
    sig.truncate(120);
    Some(sig)
}

/// 语言无关的容器查找：最近的 impl / class / trait / interface 祖先的命名节点。
fn container_from_ancestor(def_node: Node, src: &[u8]) -> Option<String> {
    let mut cur = def_node.parent();
    while let Some(node) = cur {
        let name_field = match node.kind() {
            "impl_item" => Some("type"),
            "class_declaration"
            | "abstract_class_declaration"
            | "class_definition"
            | "trait_item"
            | "interface_declaration"
            | "class_specifier" => Some("name"),
            _ => None,
        };
        if let Some(field) = name_field {
            if let Some(name_node) = node.child_by_field_name(field) {
                let text = name_node.utf8_text(src).ok()?.to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        cur = node.parent();
    }
    None
}

/// 前序遍历找第一个指定 kind 的后代节点（不含 root 自身）。
fn find_first_descendant<'a>(root: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut stack: Vec<Node> = Vec::new();
    let children: Vec<Node> = {
        let mut cursor = root.walk();
        root.children(&mut cursor).collect()
    };
    stack.extend(children);
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return Some(node);
        }
        let children: Vec<Node> = {
            let mut cursor = node.walk();
            node.children(&mut cursor).collect()
        };
        stack.extend(children);
    }
    None
}

/// 按定义节点推断 (kind, container)。容器优先走祖先，Go method 用 receiver。
fn classify(lang_id: &str, def_node: Node, src: &[u8]) -> (&'static str, Option<String>) {
    let kind = match def_node.kind() {
        "function_item"
        | "function_declaration"
        | "function_definition"
        | "generator_function_declaration" => "function",
        "method_definition" | "method_declaration" | "constructor_declaration" => "method",
        "struct_item" | "type_spec" => "struct",
        "enum_item" | "enum_declaration" => "enum",
        "trait_item" => "trait",
        "interface_declaration" => "interface",
        "class_declaration" | "abstract_class_declaration" | "class_definition" => "class",
        "record_declaration" => "class",
        "const_item" | "static_item" => "constant",
        "type_item" | "type_alias_declaration" => "type",
        // const fnExpr = () => 1 —— 函数值声明归 function
        "variable_declarator" => "function",
        _ => "symbol",
    };

    // Go method 的容器在 receiver：`func (r T) Name()`（receiver 是参数列表，
    // type_identifier 在嵌套层，需递归找）。
    if lang_id == "go" && def_node.kind() == "method_declaration" {
        if let Some(receiver) = def_node.child_by_field_name("receiver") {
            if let Some(type_node) = find_first_descendant(receiver, "type_identifier") {
                if let Ok(text) = type_node.utf8_text(src) {
                    if !text.is_empty() {
                        // 指针 receiver（*T）的 type_identifier 同名
                        return ("method", Some(text.to_string()));
                    }
                }
            }
        }
    }

    // type_spec 的 struct/interface 由子节点定。
    if lang_id == "go" && def_node.kind() == "type_spec" {
        if let Some(ty) = def_node.child_by_field_name("type") {
            let kind = match ty.kind() {
                "struct_type" => "struct",
                "interface_type" => "interface",
                _ => "type",
            };
            return (kind, None);
        }
    }

    // Rust function 在 impl 内 → method；Python function 在 class 内 → method。
    if (lang_id == "rust" && def_node.kind() == "function_item")
        || (lang_id == "python" && def_node.kind() == "function_definition")
    {
        let container = container_from_ancestor(def_node, src);
        let kind = if container.is_some() {
            "method"
        } else {
            "function"
        };
        return (kind, container);
    }

    (kind, container_from_ancestor(def_node, src))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::lang::detect_language;

    fn extract_one(lang_ext: &str, code: &str) -> FileExtraction {
        let lang = detect_language(lang_ext).unwrap();
        extract(lang, code).unwrap()
    }

    #[test]
    fn rust_symbols_and_containers() {
        let code = r#"
struct Foo { a: u32 }
enum Color { Red }
trait Speak { fn speak(&self); }
impl Foo {
    fn build(&self) -> u32 { self.a }
    fn helper(&self) {}
    fn use_free(&self) -> u32 { free_fn(1) }
}
fn free_fn(x: u32) -> u32 { x }
const MAX: u32 = 3;
"#;
        let out = extract_one("rs", code);
        let names: Vec<(&str, &str)> = out
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.kind))
            .collect();
        assert!(names.contains(&("Foo", "struct")));
        assert!(names.contains(&("Color", "enum")));
        assert!(names.contains(&("Speak", "trait")));
        assert!(names.contains(&("free_fn", "function")));
        assert!(names.contains(&("MAX", "constant")));
        let build = out.symbols.iter().find(|s| s.name == "build").unwrap();
        assert_eq!(build.kind, "method");
        assert_eq!(build.container.as_deref(), Some("Foo"));
        // free_fn 的调用点 + 方法内 self.a 的字段引用
        assert!(out.refs.iter().any(|r| r.name == "free_fn" && r.is_call));
    }

    #[test]
    fn rust_call_vs_plain_ref() {
        let code = r#"
fn caller() -> u32 { helper() + 1 }
"#;
        let out = extract_one("rs", code);
        let helper = out.refs.iter().find(|r| r.name == "helper").unwrap();
        assert!(helper.is_call);
    }

    #[test]
    fn ts_symbols() {
        let code = r#"
interface User { id: string }
type Maybe<T> = T | null;
class Service {
  run(): void {}
}
export function make(): Service { return new Service(); }
const fnExpr = () => 1;
"#;
        let out = extract_one("ts", code);
        let names: Vec<(&str, &str)> = out
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.kind))
            .collect();
        assert!(names.contains(&("User", "interface")));
        assert!(names.contains(&("Maybe", "type")));
        assert!(names.contains(&("Service", "class")));
        assert!(names.contains(&("run", "method")));
        assert!(names.contains(&("make", "function")));
        assert!(names.contains(&("fnExpr", "function")));
        // new Service() 是构造调用
        assert!(out.refs.iter().any(|r| r.name == "Service" && r.is_call));
    }

    #[test]
    fn python_methods() {
        let code = r#"
class Robot:
    def walk(self, steps):
        return helper(steps)
def helper(n):
    return n
"#;
        let out = extract_one("py", code);
        let walk = out.symbols.iter().find(|s| s.name == "walk").unwrap();
        assert_eq!(walk.kind, "method");
        assert_eq!(walk.container.as_deref(), Some("Robot"));
        let helper_def = out.symbols.iter().find(|s| s.name == "helper").unwrap();
        assert_eq!(helper_def.kind, "function");
        // walk 体内调用 helper（夹具首行为空行，调用在第 4 行）
        let call = out.refs.iter().find(|r| r.name == "helper").unwrap();
        assert!(call.is_call);
        assert_eq!(call.line, 4);
    }

    #[test]
    fn go_receiver_and_types() {
        let code = r#"
package main

type Store struct { n int }
type Repo interface { Get(i int) error }

func (s *Store) Get(i int) error { return nil }
func New() *Store { return &Store{} }
"#;
        let out = extract_one("go", code);
        let store = out.symbols.iter().find(|s| s.name == "Store").unwrap();
        assert_eq!(store.kind, "struct");
        let repo = out.symbols.iter().find(|s| s.name == "Repo").unwrap();
        assert_eq!(repo.kind, "interface");
        let get = out.symbols.iter().find(|s| s.name == "Get").unwrap();
        assert_eq!(get.kind, "method");
        assert_eq!(get.container.as_deref(), Some("Store"));
        let new = out.symbols.iter().find(|s| s.name == "New").unwrap();
        assert_eq!(new.kind, "function");
    }

    #[test]
    fn java_classes_and_methods() {
        let code = r#"
public class Foo {
    public int bar(int x) { return helper(x); }
    private static int helper(int x) { return x; }
}
interface Echo { int send(int x); }
"#;
        let out = extract_one("java", code);
        let foo = out.symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(foo.kind, "class");
        let bar = out.symbols.iter().find(|s| s.name == "bar").unwrap();
        assert_eq!(bar.kind, "method");
        assert_eq!(bar.container.as_deref(), Some("Foo"));
        let echo = out.symbols.iter().find(|s| s.name == "Echo").unwrap();
        assert_eq!(echo.kind, "interface");
        assert!(out.refs.iter().any(|r| r.name == "helper" && r.is_call));
    }

    #[test]
    fn definition_site_not_a_reference() {
        let code = r#"
fn my_func() {}
"#;
        let out = extract_one("rs", code);
        // my_func 的名字节点是定义点，不应计引用
        assert!(!out.refs.iter().any(|r| r.name == "my_func"));
    }

    #[test]
    fn syntax_error_recovers() {
        let code = "fn broken( {}\nfn ok() {}\n";
        let out = extract_one("rs", code);
        assert!(out.symbols.iter().any(|s| s.name == "ok"));
    }
}
