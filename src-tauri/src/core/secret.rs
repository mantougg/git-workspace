use std::sync::OnceLock;

use regex::Regex;

/// Category of a detected secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretKind {
    AwsKey,
    GithubToken,
    Jwt,
    PrivateKey,
    Password,
    DatabaseUrl,
}

impl SecretKind {
    pub fn label(&self) -> &'static str {
        match self {
            SecretKind::AwsKey => "AWS Access Key",
            SecretKind::GithubToken => "GitHub Token",
            SecretKind::Jwt => "JWT",
            SecretKind::PrivateKey => "Private Key",
            SecretKind::Password => "Password",
            SecretKind::DatabaseUrl => "Database URL",
        }
    }
}

/// A located secret match within a text.
#[derive(Debug, Clone)]
pub struct SecretFinding {
    pub kind: SecretKind,
    pub start: usize,
    pub end: usize,
}

/// Compiled detection patterns, built once.
fn regexes() -> &'static [(SecretKind, Regex)] {
    static RE: OnceLock<Vec<(SecretKind, Regex)>> = OnceLock::new();
    RE.get_or_init(|| {
        vec![
            (SecretKind::AwsKey, Regex::new(r#"AKIA[0-9A-Z]{16}"#).unwrap()),
            (
                SecretKind::GithubToken,
                Regex::new(r#"(?i)(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,}"#).unwrap(),
            ),
            (
                SecretKind::Jwt,
                Regex::new(r#"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"#).unwrap(),
            ),
            (
                SecretKind::PrivateKey,
                Regex::new(r#"-----BEGIN [A-Z ]*PRIVATE KEY-----"#).unwrap(),
            ),
            (
                SecretKind::DatabaseUrl,
                Regex::new(r#"(?i)(postgres|postgresql|mysql|mongodb|redis)://[^\s"']+"#).unwrap(),
            ),
            (
                SecretKind::Password,
                Regex::new(r#"(?i)(password|passwd|pwd|secret)\s*[:=]\s*[^\s"']+"#).unwrap(),
            ),
        ]
    })
}

/// Scan `text` for known secret patterns. Returns findings sorted by position.
pub fn scan_secrets(text: &str) -> Vec<SecretFinding> {
    let mut findings: Vec<SecretFinding> = Vec::new();
    for (kind, re) in regexes() {
        for m in re.find_iter(text) {
            findings.push(SecretFinding {
                kind: kind.clone(),
                start: m.start(),
                end: m.end(),
            });
        }
    }
    findings.sort_by_key(|f| f.start);
    findings
}

/// Replace every detected secret with `***`. Used for log redaction and the
/// optional "mask" path before sending to external services.
pub fn mask_secrets(text: &str) -> String {
    let findings = scan_secrets(text);
    if findings.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for f in findings {
        if f.start < last {
            continue; // skip overlapping matches
        }
        out.push_str(&text[last..f.start]);
        out.push_str("***");
        last = f.end;
    }
    out.push_str(&text[last..]);
    out
}

/// Whether an environment variable name is sensitive enough to redact across
/// IPC/UI boundaries. Keep this key policy next to the shared T-08 secret
/// scanner so Runtime config and logs cannot drift into separate rules.
pub fn is_sensitive_environment_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    ["PASSWORD", "TOKEN", "SECRET", "PRIVATE_KEY", "API_KEY"]
        .iter()
        .any(|needle| upper.contains(needle))
}

/// Whether a path (or bare file name) must never be committed without
/// explicit confirmation: env files, private keys, credential bundles.
pub fn is_forbidden_file(path: &str) -> bool {
    let name = path.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();

    if matches!(
        lower.as_str(),
        ".env"
            | ".env.local"
            | ".env.production"
            | "credentials.json"
            | "secrets.json"
            | "id_rsa"
            | "id_ed25519"
            | "id_dsa"
            | "id_ecdsa"
    ) {
        return true;
    }
    lower.ends_with(".pem") || lower.ends_with(".key") || lower.ends_with(".p12") || lower.ends_with(".pfx")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_key() {
        let findings = scan_secrets("const key = \"AKIAIOSFODNN7EXAMPLE\";");
        assert!(findings.iter().any(|f| f.kind == SecretKind::AwsKey));
    }

    #[test]
    fn detects_jwt() {
        let text = "token=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let findings = scan_secrets(text);
        assert!(findings.iter().any(|f| f.kind == SecretKind::Jwt));
    }

    #[test]
    fn detects_private_key() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMII...";
        let findings = scan_secrets(text);
        assert!(findings.iter().any(|f| f.kind == SecretKind::PrivateKey));
    }

    #[test]
    fn detects_password_assignment() {
        let findings = scan_secrets("password=supersecret123");
        assert!(findings.iter().any(|f| f.kind == SecretKind::Password));
    }

    #[test]
    fn mask_replaces_secrets() {
        let masked = mask_secrets("password=supersecret123");
        assert!(!masked.contains("supersecret123"));
        assert!(masked.contains("***"));
    }

    #[test]
    fn mask_leaves_clean_text_untouched() {
        let text = "just a normal sentence";
        assert_eq!(mask_secrets(text), text);
    }

    #[test]
    fn sensitive_environment_keys_are_detected() {
        assert!(is_sensitive_environment_key("DB_PASSWORD"));
        assert!(is_sensitive_environment_key("api_token"));
        assert!(!is_sensitive_environment_key("SERVER_PORT"));
    }

    #[test]
    fn forbidden_files_are_detected() {
        for p in [
            ".env",
            ".env.local",
            "config/id_rsa",
            "certs/server.pem",
            "credentials.json",
            "a/b/c/api.key",
        ] {
            assert!(is_forbidden_file(p), "{} should be forbidden", p);
        }
        for p in ["src/main.rs", "README.md", "package.json"] {
            assert!(!is_forbidden_file(p), "{} should be allowed", p);
        }
    }
}
