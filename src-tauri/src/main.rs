#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod winbox;

// WinBox 实例
lazy_static::lazy_static! {
    static ref WINBOX: winbox::WinBox = {
        // 依次探测 exe 所在目录、其各级上级目录、当前工作目录及其上级，取第一个含 busybox 的 winbox/
        // 覆盖：生产（exe 旁）、tauri dev（exe 在 src-tauri/target/debug/，cwd 为 src-tauri/，
        //       项目根的 winbox/ 在 exe 的第三级父目录）
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                for anc in dir.ancestors() {
                    // 跳过 target/ 目录下的资源拷贝（bundle.resources 会拷一份 winbox-dist 到 target 里）。
                    // 开发期应优先使用项目根的真实 winbox/；发行安装包则直接命中 exe 旁的 winbox/。
                    if anc.components().any(|c| c.as_os_str() == "target") {
                        continue;
                    }
                    candidates.push(anc.join("winbox"));
                }
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            for anc in cwd.ancestors() {
                candidates.push(anc.join("winbox"));
            }
        }
        let root = candidates
            .iter()
            .find(|p| p.join("bin").join("busybox.exe").exists())
            .cloned()
            .or_else(|| candidates.first().cloned())
            .unwrap_or_else(|| std::path::PathBuf::from("winbox"));
        winbox::WinBox::new(root)
    };
}

// 执行命令：与 winbox.bat 行为一致，原样交给 busybox sh 解析（PATH 注入后 python/node/npm 自然可用）
#[tauri::command]
fn run_command(command: String) -> Result<String, String> {
    WINBOX.run(command.trim())
}

// 常驻交互 shell 会话（ConPTY）
#[tauri::command]
fn shell_start(app: tauri::AppHandle, session_id: String, rows: u16, cols: u16) -> Result<(), String> {
    WINBOX.shell_start(&session_id, app, rows, cols)
}

#[tauri::command]
fn shell_write(session_id: String, data: String) -> Result<(), String> {
    WINBOX.shell_write(&session_id, &data)
}

#[tauri::command]
fn shell_resize(session_id: String, rows: u16, cols: u16) -> Result<(), String> {
    WINBOX.shell_resize(&session_id, rows, cols)
}

#[tauri::command]
fn shell_stop(session_id: String) -> Result<(), String> {
    WINBOX.shell_stop(&session_id);
    Ok(())
}

#[tauri::command]
fn shell_list() -> Result<Vec<String>, String> {
    Ok(WINBOX.shell_list())
}

#[tauri::command]
fn agent_installed(agent: String) -> Result<bool, String> {
    Ok(WINBOX.agent_installed(&agent))
}

#[tauri::command]
async fn agent_install(app: tauri::AppHandle, agent: String) -> Result<(), String> {
    WINBOX.agent_install(app, &agent)
}

#[tauri::command]
async fn agent_uninstall(app: tauri::AppHandle, agent: String) -> Result<(), String> {
    WINBOX.agent_uninstall(app, &agent)
}

// 返回 Windows 显示语言（前端文案本地化）：zh-CN / en-US
// GetUserDefaultUILanguage = 用户在「设置 → 语言 → Windows 显示语言」中选择的语言
#[tauri::command]
fn get_os_language() -> String {
    use winapi::um::winnt::{LANG_CHINESE, LANG_CHINESE_TRADITIONAL, LANG_ENGLISH};
    use winapi::um::winnls::GetUserDefaultUILanguage;
    let langid = unsafe { GetUserDefaultUILanguage() };
    match langid & 0x3FF {
        LANG_CHINESE | LANG_CHINESE_TRADITIONAL => "zh-CN".to_string(),
        LANG_ENGLISH => "en-US".to_string(),
        _ => "en-US".to_string(),
    }
}

// ---- 系统字体枚举（设置面板字体下拉） ----
static FONT_NAMES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

unsafe extern "system" fn enum_font_proc(
    lplf: *const winapi::um::wingdi::LOGFONTW,
    _lptm: *const winapi::um::wingdi::TEXTMETRICW,
    _font_type: winapi::ctypes::c_ulong,
    _lparam: isize,
) -> winapi::ctypes::c_int {
    let name = {
        let face = (*lplf).lfFaceName;
        let len = face.iter().position(|&c| c == 0).unwrap_or(face.len());
        String::from_utf16_lossy(&face[..len])
    };
    // 过滤空名与 @ 前缀（@字体是 Windows 的竖排变体，如 @微软雅黑/@fixedsys，
    // CSS font-family 不接受，选了也不会生效）
    if !name.is_empty() && !name.starts_with('@') {
        if let Ok(mut v) = FONT_NAMES.lock() {
            if !v.contains(&name) {
                v.push(name);
            }
        }
    }
    1 // 继续枚举
}

// 返回当前系统安装的全部字体族名（排序去重）
#[tauri::command]
fn list_fonts() -> Vec<String> {
    use winapi::um::wingdi::{EnumFontFamiliesExW, LOGFONTW};
    use winapi::um::winuser::{GetDC, ReleaseDC};
    unsafe {
        if let Ok(mut v) = FONT_NAMES.lock() {
            v.clear();
        }
        let hdc = GetDC(std::ptr::null_mut());
        if !hdc.is_null() {
            let mut lf: LOGFONTW = std::mem::zeroed();
            lf.lfCharSet = 1; // DEFAULT_CHARSET：枚举全部字符集
            EnumFontFamiliesExW(hdc, &mut lf, Some(enum_font_proc), 0, 0);
            ReleaseDC(std::ptr::null_mut(), hdc);
        }
    }
    let mut names = FONT_NAMES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    names.sort();
    names.dedup();
    names
}

// 最小化窗口
#[tauri::command]
fn minimize_window(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

// 最大化窗口
#[tauri::command]
fn maximize_window(window: tauri::Window) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

// 关闭窗口
#[tauri::command]
fn close_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            run_command,
            shell_start,
            shell_write,
            shell_resize,
            shell_stop,
            shell_list,
            agent_installed,
            agent_install,
            agent_uninstall,
            get_os_language,
            list_fonts,
            minimize_window,
            maximize_window,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running feier-one");
}
