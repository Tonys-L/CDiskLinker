// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 启动时检测管理员权限，未提权则自我提权重启
    if !cdisklinker_lib::win_util::check_administrator_privileges() {
        let _ = cdisklinker_lib::win_util::elevate_self();
        // 如果提权失败则继续运行（某些功能可能受限）
    }
    cdisklinker_lib::run()
}
