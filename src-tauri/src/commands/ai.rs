use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::diff;
use crate::error::{AppError, AppResult};

/// AI code review result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    pub summary: String,
    pub issues: Vec<ReviewIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewIssue {
    pub severity: String,  // "high", "medium", "low"
    pub category: String,  // "bug", "security", "optimization"
    pub file: String,
    pub description: String,
}

/// AI search result entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub repo_path: String,
    pub file_path: String,
    pub snippet: String,
    pub rank: f64,
}

/// Perform an AI code review on the working directory diff.
///
/// Sends the diff to an external AI API (OpenAI-compatible) for analysis.
/// The API key must be provided by the frontend (stored in user settings).
#[tauri::command]
pub async fn ai_review(
    repo_path: String,
    api_key: String,
    api_url: Option<String>,
) -> AppResult<ReviewResult> {
    // Get the working directory diff
    let file_diffs = diff::get_workdir_diff(Path::new(&repo_path))?;

    if file_diffs.is_empty() {
        return Ok(ReviewResult {
            summary: "No changes to review.".to_string(),
            issues: vec![],
        });
    }

    // Build the diff text for the AI
    let mut diff_text = String::new();
    for file in &file_diffs {
        diff_text.push_str(&format!("--- {} ({})\n", file.new_path, file.status));
        for hunk in &file.hunks {
            for line in &hunk.lines {
                let prefix = match line.line_type.as_str() {
                    "add" => "+",
                    "delete" => "-",
                    _ => " ",
                };
                diff_text.push_str(&format!("{}{}\n", prefix, line.content));
            }
        }
        diff_text.push('\n');
    }

    // Limit diff text size to avoid token limits
    if diff_text.len() > 10000 {
        diff_text = diff_text.chars().take(10000).collect();
        diff_text.push_str("\n... (truncated)\n");
    }

    // Construct the prompt
    let prompt = format!(
        "Review the following git diff. Identify bug risks, security issues, \
        and optimization suggestions. Return the result as JSON with fields: \
        \"summary\" (string), \"issues\" (array of objects with \"severity\" \
        (\"high\"/\"medium\"/\"low\"), \"category\" (\"bug\"/\"security\"/\"optimization\"), \
        \"file\" (string), \"description\" (string)).\n\nDiff:\n{}",
        diff_text
    );

    // Call the AI API
    let url = api_url.unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [
                {"role": "system", "content": "You are a code reviewer. Respond only with JSON."},
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.3
        }))
        .send()
        .await
        .map_err(|e| AppError::Other(format!("AI API request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Other(format!(
            "AI API returned {}: {}",
            status, body
        )));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Other(format!("Failed to parse AI response: {}", e)))?;

    // Extract the content from the response
    let content = response_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("{}");

    // Parse the AI response as our ReviewResult
    let result: ReviewResult = serde_json::from_str(content).unwrap_or(ReviewResult {
        summary: content.to_string(),
        issues: vec![],
    });

    Ok(result)
}

/// Build the code search index for a repository.
/// Scans all non-binary files and writes their content to the FTS5 index.
#[tauri::command]
pub fn build_code_index(
    repo_path: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<()> {
    use rusqlite::params;
    use std::fs;
    use walkdir::WalkDir;

    let repo_path = Path::new(&repo_path);

    // Delete existing index entries for this repo
    {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        conn.execute(
            "DELETE FROM code_index WHERE repo_path = ?1",
            params![repo_path.to_string_lossy().to_string()],
        )?;
    }

    // Scan files
    let skip_dirs = [
        "node_modules",
        "target",
        "dist",
        "build",
        ".git",
        "__pycache__",
        ".next",
        ".nuxt",
        "vendor",
        ".venv",
    ];

    let mut walker = WalkDir::new(repo_path)
        .into_iter();

    let mut batch_count = 0;
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

    while let Some(Ok(entry)) = walker.next() {
        if entry.file_type().is_dir() {
            let name = entry.file_name().to_string_lossy();
            if skip_dirs.contains(&name.as_ref()) {
                walker.skip_current_dir();
            }
            continue;
        }

        let path = entry.path();
        let relative = match path.strip_prefix(repo_path) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        // Skip binary file extensions
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let text_exts = [
            "rs", "go", "py", "js", "ts", "tsx", "jsx", "vue", "java",
            "kt", "c", "cpp", "h", "hpp", "cs", "rb", "php", "swift",
            "sql", "json", "yaml", "yml", "toml", "xml", "html", "css",
            "scss", "less", "md", "txt", "sh", "bash", "zsh", "fish",
            "lua", "r", "scala", "dart", "gradle", "dockerfile",
        ];
        if !text_exts.contains(&ext) && ext != "" {
            continue;
        }

        // Read file content (limit size)
        if let Ok(metadata) = fs::metadata(path) {
            if metadata.len() > 100_000 {
                continue; // Skip large files
            }
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // Skip binary files
        };

        // Insert into FTS5 index
        let repo_path_str = repo_path.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO code_index (content, repo_path, file_path) VALUES (?1, ?2, ?3)",
            params![content, repo_path_str, relative],
        )?;

        batch_count += 1;
        if batch_count % 100 == 0 {
            log::debug!("Indexed {} files for {:?}", batch_count, repo_path);
        }
    }

    log::info!("Code index built: {} files for {:?}", batch_count, repo_path);
    Ok(())
}

/// Search the code index for matching files.
#[tauri::command]
pub fn ai_search(
    query: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<Vec<SearchResult>> {
    use rusqlite::params;

    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

    // FTS5 MATCH query
    // Sanitize the query for FTS5 (escape special characters)
    let sanitized = query
        .replace('"', "\"\"")
        .replace('*', "")
        .replace(':', "");

    let fts_query = format!("\"{}\"", sanitized);

    let mut stmt = conn.prepare(
        "SELECT repo_path, file_path, content, rank \
         FROM code_index \
         WHERE code_index MATCH ?1 \
         ORDER BY rank \
         LIMIT 50",
    )?;

    let results = stmt
        .query_map(params![fts_query], |row| {
            let repo_path: String = row.get(0)?;
            let file_path: String = row.get(1)?;
            let content: String = row.get(2)?;
            let rank: f64 = row.get(3)?;

            // Extract a snippet around the first match
            let snippet = if let Some(pos) = content.to_lowercase().find(&query.to_lowercase()) {
                let start = pos.saturating_sub(50);
                let end = (pos + query.len() + 50).min(content.len());
                let snip = &content[start..end];
                format!("...{}...", snip.trim())
            } else {
                content.chars().take(100).collect()
            };

            Ok(SearchResult {
                repo_path,
                file_path,
                snippet,
                rank,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Clear the code index for a specific repository.
#[tauri::command]
pub fn clear_code_index(
    repo_path: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<()> {
    use rusqlite::params;

    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

    conn.execute(
        "DELETE FROM code_index WHERE repo_path = ?1",
        params![repo_path],
    )?;

    Ok(())
}
