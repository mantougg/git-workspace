use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::runtime::config::MASKED_VALUE;
use crate::runtime::launch::VecEventSink;

fn temp_root(tag: &str) -> PathBuf {
    let root = crate::test_support::temp_root("gw_r11", tag);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn test_limits() -> LogLimits {
    LogLimits {
        segment_max_bytes: 1024 * 1024,
        segments_kept: 2,
        ring_max_lines: 100,
        ring_max_bytes: 16 * 1024,
        aggregate_interval: Duration::from_millis(20),
        batch_max_lines: 16,
    }
}

fn open(
    engine: &RuntimeLogEngine,
    root: &Path,
    process_id: i64,
    secrets: Vec<String>,
    events: &Arc<VecEventSink>,
) -> Arc<LogSession> {
    engine
        .open_session(root, "app", process_id, secrets, events.clone())
        .unwrap()
}

fn log_dir(root: &Path) -> PathBuf {
    root.join(".gitworkspace").join("logs").join("app")
}

/// 全部段文件内容（最旧 → 最新拼接）。
fn persisted(root: &Path, process_id: i64, keep: u32) -> String {
    segment_paths(&log_dir(root), process_id, keep)
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect::<Vec<_>>()
        .join("")
}

fn collected_lines(events: &VecEventSink, process_id: i64) -> Vec<LogLine> {
    events
        .collected()
        .into_iter()
        .flat_map(|event| match event {
            RuntimeEvent::Logs {
                process_id: id,
                lines,
                ..
            } if id == process_id => lines,
            _ => Vec::new(),
        })
        .collect()
}

/// 验收标准「磁盘日志文件无未脱敏 secret」+ 脱敏规则覆盖。
#[test]
fn capture_masks_secrets_before_persisting() {
    let root = temp_root("mask");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let session = open(&engine, &root, 1, vec!["s3cret-value".into()], &events);
    session.log(LogPhase::Run, OutputStream::Stdout, "password=123456");
    session.log(
        LogPhase::Run,
        OutputStream::Stdout,
        "connecting with s3cret-value",
    );
    session.log(
        LogPhase::Run,
        OutputStream::Stdout,
        "2026-08-23 12:00:00.123  INFO 1 --- [main] c.e.App : ok",
    );
    engine.finish_session(1);

    let on_disk = persisted(&root, 1, test_limits().segments_kept);
    assert!(!on_disk.contains("123456"), "key=value 明文不得落盘");
    assert!(!on_disk.contains("s3cret-value"), "环境变量值明文不得落盘");
    assert!(on_disk.contains(MASKED_VALUE), "环境值应替换为掩码");
    assert!(on_disk.contains("c.e.App : ok"), "普通行原样保留");

    // 事件批次中的行同样已脱敏，且级别被解析。
    let lines = collected_lines(&events, 1);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| !l.line.contains("123456")));
    assert_eq!(lines[2].level, Some(LogLevel::Info));
    let _ = std::fs::remove_dir_all(&root);
}

/// 验收标准「高频输出下 UI 保持响应（事件聚合生效）」：批次有界、
/// 行不丢、序不乱、环形缓冲有界。
#[test]
fn flood_is_aggregated_and_ring_stays_bounded() {
    let root = temp_root("flood");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let session = open(&engine, &root, 2, vec![], &events);
    for i in 0..5000 {
        session.log(
            LogPhase::Run,
            OutputStream::Stdout,
            &format!("flood line {i}"),
        );
    }
    engine.finish_session(2);

    let lines = collected_lines(&events, 2);
    assert_eq!(lines.len(), 5000, "聚合不得丢行");
    let batch_count = events
        .collected()
        .iter()
        .filter(|e| matches!(e, RuntimeEvent::Logs { process_id: 2, .. }))
        .count();
    assert!(
        batch_count <= 5000 / 16 + 8,
        "每事件 ≤16 行 + 少量周期边界批次: {batch_count}"
    );
    let seqs: Vec<u64> = lines.iter().map(|l| l.seq).collect();
    assert_eq!(seqs, (1..=5000).collect::<Vec<_>>(), "序号连续有序");

    // 环形缓冲有界（ring_max_lines = 100）。
    assert_eq!(session.tail(1000).len(), 100);
    // 落盘完整。
    let on_disk = persisted(&root, 2, test_limits().segments_kept);
    assert_eq!(on_disk.lines().count(), 5000);
    let _ = std::fs::remove_dir_all(&root);
}

/// 验收标准「实时可见」：稀疏行在聚合间隔内推送，不等进程结束。
#[test]
fn trickle_line_is_emitted_promptly() {
    let root = temp_root("trickle");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let session = open(&engine, &root, 3, vec![], &events);
    session.log(LogPhase::Run, OutputStream::Stdout, "lonely line");

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if !collected_lines(&events, 3).is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "稀疏行应在聚合间隔内推送");
        std::thread::sleep(Duration::from_millis(10));
    }
    engine.finish_session(3);
    let _ = std::fs::remove_dir_all(&root);
}

/// 「日志文件滚动切分与容量上限」：超限时移位、删除最旧段。
#[test]
fn rotation_splits_and_caps_segments() {
    let root = temp_root("rotate");
    let events = Arc::new(VecEventSink::default());
    let mut limits = test_limits();
    limits.segment_max_bytes = 64; // 每段约 3 行
    limits.segments_kept = 2;
    let engine = RuntimeLogEngine::with_limits(limits);
    let session = open(&engine, &root, 4, vec![], &events);
    for i in 0..20 {
        session.log(
            LogPhase::Run,
            OutputStream::Stdout,
            &format!("line-{i:02}-abcdefghij"),
        );
    }
    engine.finish_session(4);

    let dir = log_dir(&root);
    assert!(dir.join("4.log").is_file());
    assert!(dir.join("4.1.log").is_file());
    assert!(dir.join("4.2.log").is_file());
    assert!(!dir.join("4.3.log").exists(), "超出保留数的最旧段被删除");
    let total: usize = segment_paths(&dir, 4, 2)
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap().lines().count())
        .sum();
    assert!(total < 20, "滚动切分后容量有上限（最旧行被淘汰）");
    let current = std::fs::read_to_string(dir.join("4.log")).unwrap();
    assert!(current.contains("line-19"), "最新行在当前段");
    let _ = std::fs::remove_dir_all(&root);
}

/// 验收标准「级别过滤/搜索可用」。
#[test]
fn search_filters_by_query_and_min_level() {
    let root = temp_root("search");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let session = open(&engine, &root, 5, vec![], &events);
    for line in [
        "[INFO] booting",
        "[WARN] disk low",
        "[ERROR] boom happened",
        "    at com.example.Trace",
    ] {
        session.log(LogPhase::Run, OutputStream::Stdout, line);
    }
    engine.finish_session(5);

    let all = engine.search(&root, "app", 5, &LogFilter::default()).unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0].level, Some(LogLevel::Info));
    assert_eq!(all[1].level, Some(LogLevel::Warn));
    assert_eq!(all[2].level, Some(LogLevel::Error));
    assert_eq!(all[3].level, None, "未识别行降级为原文");
    assert_eq!(all[0].line_number, 1);

    let filtered = engine
        .search(
            &root,
            "app",
            5,
            &LogFilter {
                min_level: Some(LogLevel::Warn),
                ..Default::default()
            },
        )
        .unwrap();
    let texts: Vec<&str> = filtered.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(
        texts,
        ["[WARN] disk low", "[ERROR] boom happened", "    at com.example.Trace"],
        "级别过滤不淘汰，无级别行（stack trace 续行）保持可见"
    );

    let queried = engine
        .search(
            &root,
            "app",
            5,
            &LogFilter {
                query: Some("boom".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(queried.len(), 1);

    let limited = engine
        .search(
            &root,
            "app",
            5,
            &LogFilter {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(limited.len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// 验收标准「导出内容与显示一致」。
#[test]
fn export_writes_exactly_what_search_displays() {
    let root = temp_root("export");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let session = open(&engine, &root, 6, vec![], &events);
    for line in ["[INFO] booting", "[WARN] disk low", "[ERROR] boom happened"] {
        session.log(LogPhase::Run, OutputStream::Stdout, line);
    }
    engine.finish_session(6);

    let filter = LogFilter {
        min_level: Some(LogLevel::Warn),
        ..Default::default()
    };
    let shown = engine.search(&root, "app", 6, &filter).unwrap();
    let dest = root.join("export.txt");
    let outcome = engine.export(&root, "app", 6, &filter, &dest).unwrap();
    let exported: Vec<String> = std::fs::read_to_string(&dest)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    let shown_texts: Vec<String> = shown.into_iter().map(|e| e.text).collect();
    assert_eq!(outcome.lines as usize, shown_texts.len());
    assert_eq!(exported, shown_texts, "导出内容必须与显示一致");
    let _ = std::fs::remove_dir_all(&root);
}

/// 验收标准「进程结束后日志完整保留、可回查」+ 清空。
#[test]
fn logs_remain_queryable_after_process_ends() {
    let root = temp_root("replay");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let session = open(&engine, &root, 7, vec![], &events);
    for i in 0..5 {
        session.log(LogPhase::Run, OutputStream::Stdout, &format!("line {i}"));
    }
    engine.finish_session(7);
    assert!(session.is_finished());

    let tail = engine.tail(&root, "app", 7, 2).unwrap();
    let texts: Vec<&str> = tail.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(texts, ["line 3", "line 4"]);
    assert_eq!(engine.search(&root, "app", 7, &LogFilter::default()).unwrap().len(), 5);

    engine.clear(&root, "app", 7).unwrap();
    assert!(
        engine
            .search(&root, "app", 7, &LogFilter::default())
            .unwrap()
            .is_empty(),
        "清空后搜索应为空"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// §36 清空：活跃会话按序经 worker 截断，清空后到达的行保留。
#[test]
fn clear_on_live_session_truncates_via_worker() {
    let root = temp_root("clear");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let session = open(&engine, &root, 8, vec![], &events);
    session.log(LogPhase::Run, OutputStream::Stdout, "before clear");
    engine.clear(&root, "app", 8).unwrap();
    session.log(LogPhase::Run, OutputStream::Stdout, "after clear");
    engine.finish_session(8);

    let on_disk = persisted(&root, 8, test_limits().segments_kept);
    assert!(!on_disk.contains("before clear"));
    assert!(on_disk.contains("after clear"));
    let _ = std::fs::remove_dir_all(&root);
}

/// §35：读取应用自身 application.log——只读，不修改用户文件。
#[test]
fn application_log_is_read_readonly() {
    let root = temp_root("applog");
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let app_log = root.join("application.log");
    std::fs::write(
        &app_log,
        "2026-08-23 12:00:00.123  INFO 1 --- [main] c.e.App : up\n\
             2026-08-23 12:00:01.123 ERROR 1 --- [main] c.e.App : down\n",
    )
    .unwrap();
    let before = std::fs::read_to_string(&app_log).unwrap();

    let entries = engine.search_file(&app_log, &LogFilter::default()).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].level, Some(LogLevel::Error));
    let tail = engine.tail_file(&app_log, 1).unwrap();
    assert_eq!(tail.len(), 1);
    assert!(tail[0].text.contains("down"));
    assert_eq!(std::fs::read_to_string(&app_log).unwrap(), before, "只读");

    let missing = root.join("nope.log");
    assert!(matches!(
        engine.search_file(&missing, &LogFilter::default()),
        Err(AppError::NotFound(_))
    ));
    let _ = std::fs::remove_dir_all(&root);
}

/// §37 预留接口位：注册的分析器收到已脱敏的行。
#[test]
fn registered_analyzer_receives_masked_lines() {
    struct Spy {
        seen: Mutex<Vec<String>>,
    }
    impl LogAnalyzer for Spy {
        fn analyze(&self, line: &LogLine) {
            self.seen.lock().unwrap().push(line.line.clone());
        }
    }

    let root = temp_root("spy");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let spy = Arc::new(Spy {
        seen: Mutex::new(Vec::new()),
    });
    engine.register_analyzer(spy.clone());
    let session = open(&engine, &root, 9, vec![], &events);
    session.log(LogPhase::Run, OutputStream::Stdout, "password=abc123");
    engine.finish_session(9);

    let seen = spy.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(!seen[0].contains("abc123"), "分析器不得见到明文");
    let _ = std::fs::remove_dir_all(&root);
}

/// 查询无任何日志文件的进程 → 可行动 NotFound。
#[test]
fn query_without_log_files_is_actionable_not_found() {
    let root = temp_root("none");
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let error = engine
        .search(&root, "app", 404, &LogFilter::default())
        .unwrap_err();
    assert!(matches!(error, AppError::NotFound(_)));
    assert!(error.to_string().contains("暂无日志文件"));
    let _ = std::fs::remove_dir_all(&root);
}

/// 非法 runtime 名不得逃逸日志目录（路径守卫）。
#[test]
fn runtime_name_path_escape_is_rejected() {
    let root = temp_root("guard");
    let events = Arc::new(VecEventSink::default());
    let engine = RuntimeLogEngine::with_limits(test_limits());
    let result = engine.open_session(&root, "../escape", 1, vec![], events);
    assert!(
        matches!(result, Err(AppError::RuntimeConfig(_))),
        "路径逃逸必须被拒绝"
    );
    assert!(!root.join(".gitworkspace/logs").exists());
    let _ = std::fs::remove_dir_all(&root);
}
