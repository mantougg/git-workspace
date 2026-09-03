//! POM Cache（R-01）：`path + file hash → parsed model`。
//!
//! pom 未修改时二次加载命中缓存，跳过重新解析（§99 目标：Cache Hit < 50ms）。
//! 缓存键为 `(绝对路径, 文件内容 hash)`：仅当路径相同**且**文件内容未变时命中，
//! 避免任何基于 mtime 的竞态（mtime 可被 touch 篡改而不改内容，亦可能在同毫秒被覆盖）。
//!
//! 使用 `moka` 同步缓存（与 T-03/T-04 数据缓存同库复用，不另起存储引擎）。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use moka::sync::Cache;
use serde::{Deserialize, Serialize};

use crate::maven::model::MavenProject;
use crate::maven::parser::parse_pom_file;

/// POM Cache 统计（用于 R-08 Benchmark 与诊断）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub parse_errors: u64,
}

/// `path + file hash → parsed model` 的 POM Cache。
///
/// - `get_or_parse(path)`：若缓存中存在该路径且 hash 匹配，直接返回（hit）；
///   否则重新解析并写入（miss）。
/// - 错误的 pom 返回 `Err`，不写入缓存，且 `parse_errors` 计数 +1。
pub struct PomCache {
    /// key = 规范化绝对路径字符串。
    /// value = (文件内容 hash, 解析后的 model)。
    inner: Cache<String, (String, MavenProject)>,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    hits: AtomicU64,
    misses: AtomicU64,
    parse_errors: AtomicU64,
}

impl PomCache {
    pub fn new() -> Self {
        // 容量上限：workspace 通常数十到数百个 POM；缓存条目为纯数据，
        // 释放后由 moka 淘汰。设一个保守上限，避免无界增长。
        let inner = Cache::builder().max_capacity(2_048).build();
        Self {
            inner,
            stats: Stats::default(),
        }
    }

    /// 命中或解析并缓存。
    pub fn get_or_parse(&self, path: &Path) -> Result<MavenProject, crate::maven::parser::PomParseError> {
        let key = path_key(path);
        // 读文件计算 hash（IO 必做：判定内容是否变更的唯一可靠依据）。
        let content = std::fs::read(path).map_err(|source| crate::maven::parser::PomParseError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let hash = crate::maven::parser::hex_hash(&content);

        if let Some((cached_hash, model)) = self.inner.get(&key) {
            if cached_hash == hash {
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(model);
            }
        }

        // miss：解析。
        self.stats.misses.fetch_add(1, Ordering::Relaxed);
        let model = match crate::maven::parser::parse_pom(path, &content, &hash) {
            Ok(m) => m,
            Err(e) => {
                self.stats.parse_errors.fetch_add(1, Ordering::Relaxed);
                return Err(e);
            }
        };
        self.inner.insert(key, (hash, model.clone()));
        Ok(model)
    }

    /// 同 [`get_or_parse`]，但直接从文件路径开始（等价；保留以便调用方语义清晰）。
    pub fn parse_file(&self, path: &Path) -> Result<MavenProject, crate::maven::parser::PomParseError> {
        self.get_or_parse(path)
    }

    /// 失效单条缓存（文件被删除/重命名时）。
    pub fn invalidate(&self, path: &Path) {
        self.inner.invalidate(&path_key(path));
    }

    /// 清空全部缓存。
    pub fn invalidate_all(&self) {
        self.inner.invalidate_all();
    }

    /// 当前缓存条目数。
    pub fn entry_count(&self) -> u64 {
        self.inner.run_pending_tasks();
        self.inner.entry_count()
    }

    /// 取一份统计快照。
    pub fn stats(&self) -> PomCacheStats {
        PomCacheStats {
            hits: self.stats.hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            parse_errors: self.stats.parse_errors.load(Ordering::Relaxed),
        }
    }
}

impl Default for PomCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 规范化路径为缓存键：使用绝对路径 + 统一分隔符。
fn path_key(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    abs.to_string_lossy().replace('\\', "/")
}

/// 不走缓存的直接解析（兼容旧 API / 测试）。
pub fn parse_pom_file_uncached(path: &Path) -> Result<MavenProject, crate::maven::parser::PomParseError> {
    parse_pom_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_pom(dir: &Path, artifact: &str, version: &str) -> PathBuf {
        let pom = dir.join("pom.xml");
        let content = format!(
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.test</groupId>
  <artifactId>{}</artifactId>
  <version>{}</version>
</project>"#,
            artifact, version
        );
        std::fs::write(&pom, content).unwrap();
        pom
    }

    #[test]
    fn cache_hit_on_unchanged_file() {
        let dir = std::env::temp_dir().join(format!(
            "gw_pomcache_hit_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pom = write_pom(&dir, "demo", "1.0.0");

        let cache = PomCache::new();
        let m1 = cache.get_or_parse(&pom).unwrap();
        assert_eq!(m1.artifact_id, "demo");

        let m2 = cache.get_or_parse(&pom).unwrap();
        assert_eq!(m2.artifact_id, "demo");

        let stats = cache.stats();
        assert_eq!(stats.hits, 1, "second load must be a cache hit");
        assert_eq!(stats.misses, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_miss_on_changed_content() {
        let dir = std::env::temp_dir().join(format!(
            "gw_pomcache_miss_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pom = write_pom(&dir, "demo", "1.0.0");

        let cache = PomCache::new();
        let m1 = cache.get_or_parse(&pom).unwrap();
        assert_eq!(m1.version, "1.0.0");

        // 修改内容（版本号变化）。
        std::fs::write(
            &pom,
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.test</groupId>
  <artifactId>demo</artifactId>
  <version>2.0.0</version>
</project>"#,
        )
        .unwrap();

        let m2 = cache.get_or_parse(&pom).unwrap();
        assert_eq!(m2.version, "2.0.0", "changed content must trigger re-parse");

        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_pom_not_cached_and_counted() {
        let dir = std::env::temp_dir().join(format!(
            "gw_pomcache_err_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pom = dir.join("pom.xml");
        std::fs::write(&pom, "<project><groupId>g</groupId></project>").unwrap();

        let cache = PomCache::new();
        assert!(cache.get_or_parse(&pom).is_err());
        assert!(cache.get_or_parse(&pom).is_err());

        let stats = cache.stats();
        assert_eq!(stats.parse_errors, 2);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 2, "errors must not be cached as hits");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalidate_removes_entry() {
        let dir = std::env::temp_dir().join(format!(
            "gw_pomcache_inv_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pom = write_pom(&dir, "demo", "1.0.0");

        let cache = PomCache::new();
        let _ = cache.get_or_parse(&pom).unwrap();
        assert_eq!(cache.entry_count(), 1);

        cache.invalidate(&pom);
        // moka 是惰性淘汰，entry_count 可能未立即归零；再访问应 miss+reparse。
        let _ = cache.get_or_parse(&pom).unwrap();
        let stats = cache.stats();
        assert_eq!(stats.misses, 2, "invalidated entry must re-parse");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
