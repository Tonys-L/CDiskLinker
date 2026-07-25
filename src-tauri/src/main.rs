// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 开发模式下跳过自动提权（提权会杀死当前进程导致 Vite 开发服务器关闭）
    // 用户可从 UI 手动提权
    #[cfg(not(debug_assertions))]
    {
        if !cdisklinker_lib::win_util::check_administrator_privileges() {
            let _ = cdisklinker_lib::win_util::elevate_self();
            // 如果提权失败则继续运行（某些功能可能受限）
        }
    }
    cdisklinker_lib::run()
}
