//! GachaNote 桌面端入口。

// 发布版在 Windows 上不弹出控制台窗口，请勿删除
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    gachanote_app_lib::run()
}
