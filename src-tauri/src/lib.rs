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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // reqwest 默认不读 Windows 系统代理，只读 HTTPS_PROXY 环境变量
            // 从注册表读取系统代理并设置环境变量，让 updater 插件能访问 GitHub Releases
            if let Some(proxy) = win_util::get_system_proxy() {
                std::env::set_var("HTTPS_PROXY", &proxy);
                std::env::set_var("HTTP_PROXY", &proxy);
            }
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
            commands::confirm_delete_source,
            commands::confirm_journal_complete,
            commands::rollback_migration_instant,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
