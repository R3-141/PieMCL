// 窗口外观控制命令：磨砂材质
//
// 优先使用 Win11 的 DWM 系统背板（DWMSBT_TRANSIENTWINDOW，轻量 Acrylic，
// 模糊远弱于老式通道）；不可用时（Win10）回退 SetWindowCompositionAttribute
// 的经典 Acrylic（着色浓度可调）。两者互斥，切换前先互相清除。

use tauri::{AppHandle, Manager};

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_DISABLED: u32 = 0;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
const ACCENT_FLAG_DRAW_ALL_BORDERS: u32 = 2;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 3842;
/// DWMSBT_TRANSIENTWINDOW：系统 Acrylic（轻磨砂）
const DWMSBT_ACRYLIC: u32 = 3;
/// DWMSBT_AUTO：关闭系统背板
const DWMSBT_AUTO: u32 = 0;

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    /// 0xAABBGGRR
    accent_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: u32,
    data: isize,
    size_of_data: usize,
}

// SetWindowCompositionAttribute 未文档化、不在 user32.lib 导入库中，运行时动态加载
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleA(name: *const u8) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> isize;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attribute: u32, value: *const u32, size: u32) -> i32;
}

/// 设置 accent 策略（经典通道）
unsafe fn set_accent(hwnd: isize, state: u32, color: u32) -> Result<(), String> {
    let func = unsafe {
        let user32 = GetModuleHandleA(b"user32.dll\0".as_ptr());
        GetProcAddress(user32, c"SetWindowCompositionAttribute".to_bytes_with_nul().as_ptr())
    };
    if func == 0 {
        return Err("当前系统不支持设置磨砂（找不到 SetWindowCompositionAttribute）".into());
    }
    let accent = AccentPolicy {
        accent_state: state,
        accent_flags: ACCENT_FLAG_DRAW_ALL_BORDERS,
        accent_color: color,
        animation_id: 0,
    };
    let data = WindowCompositionAttributeData {
        attribute: WCA_ACCENT_POLICY,
        data: &accent as *const AccentPolicy as isize,
        size_of_data: std::mem::size_of::<AccentPolicy>(),
    };
    let set_attr: unsafe extern "system" fn(isize, *const WindowCompositionAttributeData) -> i32 =
        unsafe { std::mem::transmute(func) };
    let ok = unsafe { set_attr(hwnd, &data) };
    if ok == 0 {
        return Err("设置磨砂失败（SetWindowCompositionAttribute 返回 0）".into());
    }
    Ok(())
}

/// 控制主窗口磨砂：enabled=false 关闭；开启时优先系统轻磨砂，回退经典 Acrylic。
/// 返回实际生效的通道：system（系统轻磨砂，Win11）/ classic（经典，浓度可调）/ off
#[tauri::command]
pub fn set_window_blur(
    app: AppHandle,
    enabled: bool,
    is_dark: bool,
    opacity: u8,
) -> Result<String, String> {
    #[cfg(windows)]
    {
        let win = app
            .get_webview_window("main")
            .ok_or_else(|| "主窗口不存在".to_string())?;
        let hwnd = win.hwnd().map_err(|e| format!("获取窗口句柄失败：{e}"))?.0 as isize;

        // 暗色标记：让系统背板跟随应用深浅主题
        let dark: u32 = if is_dark { 1 } else { 0 };
        unsafe { DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark, 4) };

        if !enabled {
            unsafe { set_accent(hwnd, ACCENT_DISABLED, 0)? };
            let auto = DWMSBT_AUTO;
            unsafe { DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &auto, 4) };
            return Ok("off".into());
        }

        // 先禁用经典 accent（与系统背板互斥），再尝试系统轻磨砂
        unsafe { set_accent(hwnd, ACCENT_DISABLED, 0)? };
        let acrylic = DWMSBT_ACRYLIC;
        let hr = unsafe { DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &acrylic, 4) };
        if hr == 0 {
            return Ok("system".into());
        }

        // 回退：经典 Acrylic（Win10），着色跟随主题与浓度
        let (r, g, b) = if is_dark { (23, 23, 23) } else { (243, 244, 246) };
        let color = ((opacity as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32);
        unsafe { set_accent(hwnd, ACCENT_ENABLE_ACRYLICBLURBEHIND, color)? };
        Ok("classic".into())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, enabled, is_dark, opacity);
        Ok("off".into())
    }
}
