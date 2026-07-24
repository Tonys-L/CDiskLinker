mod engine;
mod journal;
mod scanner;
pub mod win_util;
mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::check_admin,
            commands::elevate_self,
            commands::scan_disk,
            commands::scan_subdirectory,
            commands::get_disk_info,
            commands::migrate_selected,
            commands::rollback_journal,
            commands::check_crash_recovery,
            commands::check_file_locks,
            commands::kill_locking_processes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
