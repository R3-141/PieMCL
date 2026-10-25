// 防止 Windows 发布版出现额外控制台窗口，勿删
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    piemcl_lib::run()
}
