use crate::ipc::session::SessionState;
use crate::journal;
use crate::models::*;
use tauri::State;

#[tauri::command]
pub fn get_subagent_journal(
    session_id: SessionId,
    subagent_id: String,
    state: State<SessionState>,
) -> Vec<JournalEntry> {
    // MCP children use their numeric session ID as the subagent ID
    if let Ok(child_id) = subagent_id.parse::<SessionId>() {
        return state.write().get_journal(child_id);
    }

    // Native subagents: read from .jsonl files on disk
    let claude_id = {
        let m = state.read();
        m.db.get_claude_session_id(session_id).ok().flatten()
    };
    let claude_session_id = match claude_id {
        Some(id) => id,
        None => return vec![],
    };

    let projects_dir = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("projects"),
        None => return vec![],
    };

    let entries = match std::fs::read_dir(&projects_dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    for project_entry in entries.flatten() {
        let jsonl_path = project_entry
            .path()
            .join(&claude_session_id)
            .join("subagents")
            .join(format!("{}.jsonl", &subagent_id));

        if jsonl_path.exists() {
            let journal_state = journal::parse_journal(&jsonl_path, 0, None);
            let mut result = journal_state.entries;
            for entry in &mut result {
                entry.session_id = subagent_id.clone();
            }
            return result;
        }
    }

    vec![]
}

#[tauri::command]
pub fn write_file_content(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, &content).map_err(|e| format!("Failed to write file: {e}"))
}

#[tauri::command]
pub fn read_file_content(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
/// Lists every project file that the Explorer can display.
///
/// Dependency and build-cache directories are skipped intentionally, while
/// project files are scanned without an arbitrary file-count or depth cap so
/// large projects do not hide later root entries.
///
/// @param cwd Absolute path of the project root to scan.
/// @return Relative, slash-normalized file paths sorted for stable display.
/// @author ductv <ductv@getflycrm.com>
/// @since 2026-09-26
pub fn list_project_files(cwd: String) -> Vec<String> {
    use ignore::WalkBuilder;

    // Directories we never descend into — only well-known dependency/cache dirs
    const SKIP_DIRS: &[&str] = &[
        ".git",
        "node_modules",
        "vendor",
        "target",
        "__pycache__",
        ".cache",
        ".next",
        ".nuxt",
        ".svelte-kit",
        ".turbo",
        ".yarn",
        ".tox",
        ".venv",
        ".mypy_cache",
        ".pytest_cache",
        "elm-stuff",
        ".dart_tool",
        ".pub-cache",
    ];

    let mut files = Vec::new();
    let walker = WalkBuilder::new(&cwd)
        .hidden(false) // show dotfiles: .env, .dockerignore, etc.
        .git_ignore(false) // don't rely on .gitignore — we block heavy dirs ourselves
        .git_global(false)
        .git_exclude(false)
        .filter_entry(move |e| {
            if e.file_type().is_some_and(|ft| ft.is_dir()) {
                let name = e.file_name().to_string_lossy();
                !SKIP_DIRS.contains(&name.as_ref())
            } else {
                true
            }
        })
        .build();

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(&cwd) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if !rel_str.is_empty() {
                files.push(rel_str.to_string());
            }
        }
    }

    files.sort();
    files
}

#[tauri::command]
pub fn search_project_files(cwd: String, query: String, limit: Option<u32>) -> Vec<String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let cap = limit.unwrap_or(50).min(200) as usize;
    list_project_files(cwd)
        .into_iter()
        .filter(|path| path.to_lowercase().contains(&q))
        .take(cap)
        .collect()
}
