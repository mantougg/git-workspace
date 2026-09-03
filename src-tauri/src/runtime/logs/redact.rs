//! 日志脱敏管道（R-11，§77 Runtime Log Secret Mask + 全局约束 §4）。
//!
//! 规则单一来源：检测/掩码复用 T-08 [`crate::core::secret`]，敏感环境变量
//! key 判定同样走 T-08 [`is_sensitive_environment_key`][crate::core::secret::is_sensitive_environment_key]；
//! 本模块只做 Runtime 日志场景的组合（`key=value` 形态 + 敏感环境值字面量
//! 替换），R-09 构建流水线与本引擎共用同一个 [`LogRedactor`]，不另起一套。
//!
//! **约束：脱敏必须在落盘前完成**——磁盘上不得出现明文 secret（验收标准）。

use crate::runtime::config::MASKED_VALUE;

/// 收集需要掩码的敏感环境变量值（T-08 共享 key 规则；空值无掩码意义）。
pub fn sensitive_env_values(env: &[(String, String)]) -> Vec<String> {
    env.iter()
        .filter(|(key, value)| crate::core::secret::is_sensitive_environment_key(key) && !value.is_empty())
        .map(|(_, value)| value.clone())
        .collect()
}

/// 日志脱敏器：T-08 模式掩码（`password=…`、`AKIA…`、JWT 等）+ 本次
/// 运行环境里的敏感值字面量替换（环境值在日志里出现也一并打码，§77）。
///
/// `secrets` 只在内存中持有，从不序列化、不跨 IPC。
#[derive(Debug, Clone, Default)]
pub struct LogRedactor {
    secrets: Vec<String>,
}

impl LogRedactor {
    pub fn new(secrets: Vec<String>) -> Self {
        Self { secrets }
    }

    /// 从环境表构造（[`sensitive_env_values`] 的便捷组合）。
    pub fn from_env(env: &[(String, String)]) -> Self {
        Self::new(sensitive_env_values(env))
    }

    /// 脱敏一行日志，返回可落盘/可外发的文本。
    pub fn mask(&self, line: &str) -> String {
        let mut out = crate::core::secret::mask_secrets(line);
        for secret in &self.secrets {
            // 短值误伤面太大（如 "1" 会把日志打花），只掩码足够长的秘密值。
            if secret.len() >= 4 && out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), MASKED_VALUE);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_key_value_forms() {
        let redactor = LogRedactor::default();
        // key=value / key: value 形态走 T-08 模式（整段打码，无明文残留）。
        assert!(!redactor.mask("password=123456").contains("123456"));
        assert!(!redactor.mask("db passwd: hunter2").contains("hunter2"));
        assert!(!redactor
            .mask("spring.datasource.password=p@ssw0rd!")
            .contains("p@ssw0rd!"));
    }

    #[test]
    fn masks_sensitive_environment_values_anywhere_in_line() {
        let env = vec![
            ("DB_PASSWORD".to_string(), "s3cret-value".to_string()),
            ("SERVER_PORT".to_string(), "8080".to_string()), // 非敏感 key
        ];
        let redactor = LogRedactor::from_env(&env);
        let masked = redactor.mask("connecting with s3cret-value to db");
        assert!(masked.contains(MASKED_VALUE));
        assert!(!masked.contains("s3cret-value"));
        // 非敏感值原样保留。
        assert_eq!(redactor.mask("port 8080 up"), "port 8080 up");
    }

    #[test]
    fn short_secret_values_are_not_masked() {
        // 长度 < 4 的秘密值不替换（避免 "1" 之类的值打花整片日志）。
        let redactor = LogRedactor::new(vec!["abc".to_string()]);
        assert_eq!(redactor.mask("abc stays"), "abc stays");
    }

    #[test]
    fn clean_lines_pass_through_untouched() {
        let redactor = LogRedactor::from_env(&[("API_KEY".to_string(), "abcd-1234".to_string())]);
        let line = "2026-08-23 12:00:00.123  INFO 1 --- [main] c.e.App : ok";
        assert_eq!(redactor.mask(line), line);
    }
}
