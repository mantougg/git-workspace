//! Pathing JAR（F-11）：绕过 Windows 命令行长度上限。
//!
//! Windows CreateProcess 命令行上限 32767 字符；企业级项目的
//! `java -cp <数百个 jar 绝对路径>` 轻易超限（spawn 失败：os error 206
//! 「文件名或扩展名太长」）。解法与 IDEA 的「JAR manifest」缩短策略一致：
//! 生成一个只含 META-INF/MANIFEST.MF 的 stub jar，把完整 classpath 写进
//! manifest 的 `Class-Path`（空格分隔的 file: URL），启动时只传
//! `-cp pathing.jar`。
//!
//! 版本兼容性：`Class-Path` manifest 属性是 JAR 规范的基础机制，
//! JDK 8 / 11 / 17 / 21 行为一致（均实测：8 由 hussar 真实启动验证，
//! 17/21 由合成用例验证）。不能用 `@argfile`（需 JDK 9+，本项目目标含
//! Java 8 工程）。
//!
//! 产物落在 workspace 的 classpath 缓存目录（R-14 §78 只读护栏范围内），
//! 文件名取 classpath 内容哈希，classpath 不变时直接复用。

use std::path::{Path, PathBuf};

use crate::error::AppResult;
use crate::maven::parser::hex_hash;

/// 触发缩短的命令行估算阈值：Windows CreateProcess 上限 32767，留余量。
/// Unix 的 ARG_MAX 远大于此，统一阈值对全平台安全。
const COMMAND_LINE_SOFT_LIMIT: usize = 30_000;

/// 估算命令行过长时，把 classpath 收敛为 pathing jar 并返回新 classpath
/// （仅含 stub jar）；不超限则原样返回（零开销）。
pub fn shorten_if_needed(
    workspace_root: &Path,
    runtime_name: &str,
    classpath: Vec<PathBuf>,
    estimated_command_len: usize,
) -> AppResult<Vec<PathBuf>> {
    if estimated_command_len < COMMAND_LINE_SOFT_LIMIT {
        return Ok(classpath);
    }
    let dir = super::classpath::classpath_cache_dir(workspace_root, runtime_name);
    crate::runtime::guard::assert_workspace_write_path(&dir, workspace_root, "Pathing JAR")?;
    let jar = write_pathing_jar(&dir, &classpath)?;
    log::info!(
        "F-11: classpath 过长（估算 {estimated_command_len} 字符），已收敛为 pathing jar {}",
        jar.display()
    );
    Ok(vec![jar])
}

/// 估算完整启动命令长度（含 java 可执行、VM options、-cp、主类、程序参数）。
pub fn estimate_command_len(
    java_exec: &Path,
    vm_options: &[String],
    classpath: &[PathBuf],
    main_class: &str,
    program_arguments: &[String],
) -> usize {
    let cp_len = classpath
        .iter()
        .map(|p| p.to_string_lossy().len() + 1) // 路径分隔符
        .sum::<usize>();
    java_exec.to_string_lossy().len()
        + vm_options.iter().map(|o| o.len() + 1).sum::<usize>()
        + cp_len
        + main_class.len()
        + program_arguments.iter().map(|a| a.len() + 1).sum::<usize>()
        + 8 // "-cp" 与空格
}

/// 写（或复用）pathing jar：只含 META-INF/MANIFEST.MF 的 Stored zip。
fn write_pathing_jar(dir: &Path, classpath: &[PathBuf]) -> AppResult<PathBuf> {
    let fingerprint_input = classpath
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>()
        .join(";");
    let key = hex_hash(fingerprint_input.as_bytes());
    let jar_path = dir.join(format!("pathing-{}.jar", &key[..16]));
    if jar_path.is_file() {
        return Ok(jar_path);
    }
    std::fs::create_dir_all(dir)?;
    let manifest = build_manifest(classpath);
    let file = std::fs::File::create(&jar_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    use std::io::Write;
    zip.start_file("META-INF/MANIFEST.MF", options)
        .and_then(|_| {
            zip.write_all(manifest.as_bytes())
                .map_err(zip::result::ZipError::Io)
        })
        .and_then(|_| zip.finish())
        .map_err(|error| {
            crate::error::AppError::Other(format!(
                "写 pathing jar 失败 {}：{error}",
                jar_path.display()
            ))
        })?;
    Ok(jar_path)
}

/// 按 JAR 规范构造 manifest：CRLF 行尾，每行 ≤72 字节，续行前导一个空格，
/// header 段以空行结束。
fn build_manifest(classpath: &[PathBuf]) -> String {
    let urls: Vec<String> = classpath.iter().map(|p| entry_url(p)).collect();
    let class_path = format!("Class-Path: {}", urls.join(" "));
    let mut out = String::from("Manifest-Version: 1.0\r\n");
    out.push_str(&wrap72(&class_path));
    out.push_str("\r\n");
    out
}

/// 单条 classpath 项 → file: URL。目录必须以 `/` 结尾（否则被当作 jar 文件）；
/// Windows 盘符路径 `C:/x` → `file:/C:/x`；空格与非 ASCII 按 UTF-8 百分号
/// 编码（空格不编码会被当成条目分隔符）。
fn entry_url(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    if path.is_dir() && !s.ends_with('/') {
        s.push('/');
    }
    let with_scheme = if s.starts_with('/') {
        format!("file:{s}")
    } else {
        format!("file:/{s}")
    };
    percent_encode(&with_scheme)
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 按 JAR manifest 规范折行：首行 ≤72 字节，续行 = 一个空格 + ≤71 字节，
/// CRLF 结尾；不在 UTF-8 多字节字符中间断开。
fn wrap72(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::new();
    let mut idx = 0;
    let mut first = true;
    while idx < bytes.len() {
        let limit = if first { 72 } else { 71 };
        let mut end = (idx + limit).min(bytes.len());
        while end < bytes.len() && !value.is_char_boundary(end) {
            end -= 1;
        }
        if !first {
            out.push(' ');
        }
        out.push_str(&value[idx..end]);
        out.push_str("\r\n");
        idx = end;
        first = false;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_lines_respect_72_bytes_and_crlf() {
        let long: Vec<PathBuf> = (0..50)
            .map(|i| {
                PathBuf::from(format!(
                    "C:/m2/repository/some/long/dependency/path/number-{i}-with-a-long-name.jar"
                ))
            })
            .collect();
        let manifest = build_manifest(&long);
        assert!(manifest.starts_with("Manifest-Version: 1.0\r\n"));
        assert!(manifest.ends_with("\r\n\r\n"));
        for line in manifest.split("\r\n") {
            assert!(line.len() <= 72, "line too long: {line}");
        }
        // Class-Path 必然折行，续行以一个空格开头。
        assert!(manifest.contains("\r\n "), "expected continuation lines");
        assert!(manifest.contains("Class-Path: file:/C:/m2/"));
    }

    #[test]
    fn entry_url_encodes_spaces_and_marks_dirs() {
        let dir = std::env::temp_dir().join(format!(
            "gw_pathing dir+{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let url = entry_url(&dir);
        assert!(url.ends_with('/'), "目录必须以 / 结尾：{url}");
        assert!(url.contains("%20"), "空格必须百分号编码：{url}");
        assert!(url.contains("%2B"), "+ 必须百分号编码：{url}");
        assert!(url.starts_with("file:/"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortens_only_when_over_limit_and_jar_is_reusable() {
        let root = std::env::temp_dir().join(format!(
            "gw_pathing_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();

        let short_cp = vec![PathBuf::from("target/classes")];
        let unchanged = shorten_if_needed(&root, "app", short_cp.clone(), 100).unwrap();
        assert_eq!(unchanged, short_cp, "低于阈值不动");

        let long_cp: Vec<PathBuf> = (0..5)
            .map(|i| PathBuf::from(format!("D:/m2/dep-{i}.jar")))
            .collect();
        let shortened =
            shorten_if_needed(&root, "app", long_cp.clone(), COMMAND_LINE_SOFT_LIMIT).unwrap();
        assert_eq!(shortened.len(), 1);
        let jar = &shortened[0];
        assert!(jar.is_file(), "pathing jar 必须真实落盘");
        assert!(jar.starts_with(root.join(".gitworkspace")), "护栏目录内");

        // 内容寻址：同 classpath 复用同一文件。
        let again =
            shorten_if_needed(&root, "app", long_cp.clone(), COMMAND_LINE_SOFT_LIMIT).unwrap();
        assert_eq!(&again[0], jar);

        // manifest 内容包含全部条目（先展开续行再断言：折行可能从条目
        // 中间断开，这是 JAR 规范允许且必须的）。
        let file = std::fs::File::open(jar).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut content = String::new();
        use std::io::Read;
        archive
            .by_name("META-INF/MANIFEST.MF")
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        let unwrapped = content.replace("\r\n ", "");
        for i in 0..5 {
            assert!(
                unwrapped.contains(&format!("dep-{i}.jar")),
                "manifest 缺 dep-{i}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
