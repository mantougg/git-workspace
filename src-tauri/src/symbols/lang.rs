//! 语言配置：按扩展名选语法与查询（T-28）。
//!
//! 覆盖主要语言：Rust / TypeScript(.ts,.tsx) / JavaScript(.js,.jsx,.mjs,.cjs)
//! / Python / Go / Java。查询节点名与字段名以 0.23 语法 crate 为准；
//! kind 与容器（所属 impl/class/接口）在 `extract::classify` 里按定义节点
//! 推断，查询只负责捕获 `@name`（命名节点）与 `@def`（定义节点）。

use std::sync::LazyLock;

use tree_sitter::Language;

pub struct LangConfig {
    /// 语言标识（落库扩展用，当前未持久化）
    pub id: &'static str,
    pub language: Language,
    /// 多 pattern 查询：每个 pattern 捕获 `@name` + `@def`
    pub symbol_query: &'static str,
    /// 引用捕获：标识符节点 `@ref`（不含调用名节点，见 `call_query`）
    pub ref_query: &'static str,
    /// 调用名捕获：`@call`（节点与引用标识符同一节点，便于合并 is_call）
    pub call_query: &'static str,
}

const RUST_SYMBOLS: &str = r#"
(function_item name: (identifier) @name) @def
(struct_item name: (type_identifier) @name) @def
(enum_item name: (type_identifier) @name) @def
(trait_item name: (type_identifier) @name) @def
(const_item name: (identifier) @name) @def
(static_item name: (identifier) @name) @def
(type_item name: (type_identifier) @name) @def
"#;

const RUST_REFS: &str = r#"
(identifier) @ref
(type_identifier) @ref
"#;

const RUST_CALLS: &str = r#"
(call_expression function: (identifier) @call)
(call_expression function: (field_expression field: (field_identifier) @call))
(macro_invocation macro: (identifier) @call)
"#;

const JS_SYMBOLS: &str = r#"
(function_declaration name: (identifier) @name) @def
(class_declaration name: (identifier) @name) @def
(method_definition name: (property_identifier) @name) @def
(variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)]) @def
"#;

const JS_REFS: &str = r#"
(identifier) @ref
(property_identifier) @ref
"#;

const TS_SYMBOLS: &str = r#"
(function_declaration name: (identifier) @name) @def
(class_declaration name: (type_identifier) @name) @def
(abstract_class_declaration name: (type_identifier) @name) @def
(interface_declaration name: (type_identifier) @name) @def
(enum_declaration name: (identifier) @name) @def
(type_alias_declaration name: (type_identifier) @name) @def
(method_definition name: (property_identifier) @name) @def
(variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)]) @def
"#;

const TS_REFS: &str = r#"
(identifier) @ref
(property_identifier) @ref
(type_identifier) @ref
"#;

const TS_CALLS: &str = r#"
(call_expression function: (identifier) @call)
(call_expression function: (member_expression property: (property_identifier) @call))
(new_expression constructor: (identifier) @call)
(new_expression constructor: (member_expression property: (property_identifier) @call))
"#;

const PY_SYMBOLS: &str = r#"
(function_definition name: (identifier) @name) @def
(class_definition name: (identifier) @name) @def
"#;

const PY_REFS: &str = r#"
(identifier) @ref
"#;

const PY_CALLS: &str = r#"
(call function: (identifier) @call)
(call function: (attribute attribute: (identifier) @call))
"#;

const GO_SYMBOLS: &str = r#"
(function_declaration name: (identifier) @name) @def
(method_declaration name: (field_identifier) @name) @def
(type_spec name: (type_identifier) @name) @def
"#;

const GO_REFS: &str = r#"
(identifier) @ref
(field_identifier) @ref
(type_identifier) @ref
"#;

const GO_CALLS: &str = r#"
(call_expression function: (identifier) @call)
(call_expression function: (selector_expression field: (field_identifier) @call))
"#;

const JAVA_SYMBOLS: &str = r#"
(method_declaration name: (identifier) @name) @def
(constructor_declaration name: (identifier) @name) @def
(class_declaration name: (identifier) @name) @def
(interface_declaration name: (identifier) @name) @def
(enum_declaration name: (identifier) @name) @def
(record_declaration name: (identifier) @name) @def
"#;

const JAVA_REFS: &str = r#"
(identifier) @ref
"#;

const JAVA_CALLS: &str = r#"
(method_invocation name: (identifier) @call)
(object_creation_expression type: (type_identifier) @call)
"#;

macro_rules! lang {
    ($id:literal, $lang:expr, $sym:expr, $ref:expr, $call:expr) => {
        LangConfig {
            id: $id,
            language: $lang,
            symbol_query: $sym,
            ref_query: $ref,
            call_query: $call,
        }
    };
}

/// 全部受支持语言（顺序即扩展名匹配顺序语义，互不重叠）。
/// tree-sitter Language 的构造非 const，用 LazyLock 静态缓存。
pub static LANGS: LazyLock<Vec<LangConfig>> = LazyLock::new(|| {
    vec![
        lang!(
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            RUST_SYMBOLS,
            RUST_REFS,
            RUST_CALLS
        ),
        lang!(
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            TS_SYMBOLS,
            TS_REFS,
            TS_CALLS
        ),
        lang!(
            "tsx",
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            TS_SYMBOLS,
            TS_REFS,
            TS_CALLS
        ),
        lang!(
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            JS_SYMBOLS,
            JS_REFS,
            TS_CALLS
        ),
        lang!(
            "python",
            tree_sitter_python::LANGUAGE.into(),
            PY_SYMBOLS,
            PY_REFS,
            PY_CALLS
        ),
        lang!(
            "go",
            tree_sitter_go::LANGUAGE.into(),
            GO_SYMBOLS,
            GO_REFS,
            GO_CALLS
        ),
        lang!(
            "java",
            tree_sitter_java::LANGUAGE.into(),
            JAVA_SYMBOLS,
            JAVA_REFS,
            JAVA_CALLS
        ),
    ]
});

/// 按文件扩展名识别语言（无扩展名 / 未支持 → None）。
pub fn detect_language(ext: &str) -> Option<&'static LangConfig> {
    let langs = &*LANGS;
    let lang = match ext.to_ascii_lowercase().as_str() {
        "rs" => &langs[0],
        "ts" => &langs[1],
        "tsx" | "jsx" => &langs[2],
        "js" | "mjs" | "cjs" => &langs[3],
        "py" => &langs[4],
        "go" => &langs[5],
        "java" => &langs[6],
        _ => return None,
    };
    Some(lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_language_by_extension() {
        assert_eq!(detect_language("rs").unwrap().id, "rust");
        assert_eq!(detect_language("TS").unwrap().id, "typescript");
        assert_eq!(detect_language("tsx").unwrap().id, "tsx");
        assert_eq!(detect_language("jsx").unwrap().id, "tsx");
        assert_eq!(detect_language("mjs").unwrap().id, "javascript");
        assert_eq!(detect_language("py").unwrap().id, "python");
        assert_eq!(detect_language("go").unwrap().id, "go");
        assert_eq!(detect_language("java").unwrap().id, "java");
        assert!(detect_language("md").is_none());
        assert!(detect_language("").is_none());
    }

    #[test]
    fn all_queries_compile() {
        // 语法/查询随 crate 升级可能改名：启动即测，防止运行期静默失效。
        for lang in LANGS.iter() {
            tree_sitter::Query::new(&lang.language, lang.symbol_query)
                .unwrap_or_else(|e| panic!("{} symbol_query: {e}", lang.id));
            tree_sitter::Query::new(&lang.language, lang.ref_query)
                .unwrap_or_else(|e| panic!("{} ref_query: {e}", lang.id));
            tree_sitter::Query::new(&lang.language, lang.call_query)
                .unwrap_or_else(|e| panic!("{} call_query: {e}", lang.id));
        }
    }
}
