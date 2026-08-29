/// Restart the application after a downloaded update has been installed.
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}
