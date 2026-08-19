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

// ---- WebView2 最小化/还原视口修复 ----
// 现象：transparent:true 窗口最小化再还原后，WebView2 渲染视口永久卡在最小化尺寸（160x28），
// 窗口内容消失，只有重启进程能恢复。
// 微软文档要求应用在最小化时把 WebView IsVisible 置 false、还原时置 true，tauri v1 未实现
// （官方在 v2 dev 试过 PR #9246，因白屏回归被 revert #9465）。这里手动补上：
// 最小化 → IsVisible(false) 停止渲染；还原 → IsVisible(true) + SetBounds(全客户区) 直接重设视口，
// 绕过已断掉的 WM_SIZE 消息链。SetBounds 单位是 DIP，需按 scale factor 从物理像素换算。
// ---- 诊断日志：写入 %TEMP%\tinux-webview-fix.log（窗口打不开时也能复盘）----
fn log_fix(msg: impl AsRef<str>) {
    use std::io::Write;
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = std::env::temp_dir().join("tinux-webview-fix.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", millis, msg.as_ref());
    }
}

// 供心跳线程读取的顶层窗口句柄（HWND 生命周期与窗口一致，存裸值即可）
static MAIN_HWND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
// 最近一次还原尺寸（DIP），最小化时把视口拉回用（预防 160x28 缩放竞态）
static LAST_VIEW_W: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(900);
static LAST_VIEW_H: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(650);
// WndProc 观察钩子：只记录关键消息并原样转发，不改行为。
// 任务栏还原的关键是 WM_QUERYOPEN——窗口过程返回 FALSE 会静默取消还原（窗口保持最小化、无任何后续消息），
// 与"点任务栏没反应"的观察完全吻合；钩子能拿到消息级流水坐实/排除这个假设。
static OLD_WNDPROC: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe extern "system" fn wnd_proc_hook(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows::Win32::UI::WindowsAndMessaging::IsIconic;
    const WM_MOVE: u32 = 0x0003;
    const WM_SIZE: u32 = 0x0005;
    const WM_ACTIVATE: u32 = 0x0006;
    const WM_CLOSE: u32 = 0x0010;
    const WM_QUERYOPEN: u32 = 0x0013;
    const WM_SHOWWINDOW: u32 = 0x0018;
    const WM_SYSCOMMAND: u32 = 0x0112;
    let interesting =
        matches!(msg, WM_MOVE | WM_SIZE | WM_ACTIVATE | WM_CLOSE | WM_QUERYOPEN | WM_SHOWWINDOW | WM_SYSCOMMAND);
    if interesting {
        log_fix(format!(
            "WNDPROC msg=0x{:04x} wp=0x{:x} lp=0x{:x} iconic={}",
            msg,
            wparam,
            lparam,
            IsIconic(hwnd).as_bool()
        ));
    }
    let old = OLD_WNDPROC.load(std::sync::atomic::Ordering::SeqCst);
    if old == 0 {
        return 0; // 理论不会发生：只在装好钩子之后才有消息
    }
    let ret: isize =
        std::mem::transmute::<usize, unsafe extern "system" fn(windows::Win32::Foundation::HWND, u32, usize, isize) -> isize>(
            old,
        )(hwnd, msg, wparam, lparam);
    if interesting {
        log_fix(format!("WNDPROC -> 0x{:04x} ret=0x{:x}", msg, ret as usize));
    }
    ret
}

fn install_wndproc_hook(window: &tauri::Window) {
    if OLD_WNDPROC.load(std::sync::atomic::Ordering::SeqCst) != 0 {
        return;
    }
    use windows::Win32::UI::WindowsAndMessaging::{GWLP_WNDPROC, SetWindowLongPtrW};
    if let Ok(h) = window.hwnd() {
        unsafe {
            let old = SetWindowLongPtrW(h, GWLP_WNDPROC, wnd_proc_hook as *const () as isize);
            OLD_WNDPROC.store(old as usize, std::sync::atomic::Ordering::SeqCst);
            log_fix(format!("WNDPROC hook installed old=0x{:x}", old as usize));
        }
    }
}

fn window_state_str(window: &tauri::Window) -> String {
    format!(
        "inner={} scale={} minimized={} visible={} maximized={}",
        window
            .inner_size()
            .map(|s| format!("{}x{}", s.width, s.height))
            .unwrap_or_else(|_| "?".into()),
        window.scale_factor().unwrap_or(-1.0),
        window.is_minimized().unwrap_or(false),
        window.is_visible().unwrap_or(false),
        window.is_maximized().unwrap_or(false)
    )
}

// 直接从 HWND 读 OS 层状态（tao 的状态可能失真，用 winapi 交叉验证）：
// iconic=窗口是否仍处于最小化；client/win=OS 实际窗口矩形，可确认窗口本身是否卡在 160x28
fn hwnd_state_str(window: &tauri::Window) -> String {
    // 用 windows crate 的 API（与 window.hwnd() 返回类型一致），不再和 winapi 的 HWND 转换纠缠
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, IsIconic, IsWindowVisible};
    let hwnd = match window.hwnd() {
        Ok(h) => h,
        Err(_) => return "hwnd=none".into(),
    };
    unsafe {
        let mut c: RECT = std::mem::zeroed();
        let c_ok = GetClientRect(hwnd, &mut c).as_bool();
        let mut w: RECT = std::mem::zeroed();
        let w_ok = GetWindowRect(hwnd, &mut w).as_bool();
        format!(
            "hwnd=0x{:x} iconic={} vis={} client=({},{},{},{}){} win=({},{},{},{}){}",
            hwnd.0 as usize,
            IsIconic(hwnd).as_bool(),
            IsWindowVisible(hwnd).as_bool(),
            c.left, c.top, c.right, c.bottom,
            if c_ok { "" } else { " [client err]" },
            w.left, w.top, w.right, w.bottom,
            if w_ok { "" } else { " [win err]" }
        )
    }
}

fn fix_webview_after_minimize_restore(window: &tauri::Window) {
    let minimized = window.is_minimized().unwrap_or(false);
    // with_webview 的闭包要求 'static，不能借用 window——尺寸换算提前算好，闭包只捕获 Copy 值
    let (right, bottom) = if minimized {
        (0, 0)
    } else if let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
        let logical: tauri::LogicalSize<f64> = size.to_logical(scale);
        (logical.width.round() as i32, logical.height.round() as i32)
    } else {
        (0, 0)
    };
    if !minimized {
        LAST_VIEW_W.store(right, std::sync::atomic::Ordering::SeqCst);
        LAST_VIEW_H.store(bottom, std::sync::atomic::Ordering::SeqCst);
    }
    let state = window_state_str(window);
    let hwnd_state = hwnd_state_str(window);
    let r = window.with_webview(move |webview| {
        let controller = webview.controller();
        let mut out = format!(
            "SetIsVisible({})={}; ",
            !minimized,
            unsafe { controller.SetIsVisible(!minimized).is_ok() }
        );
        // 读旧视口矩形（诊断用）
        let mut bounds = unsafe { std::mem::zeroed() };
        let b_ok = unsafe { controller.Bounds(&mut bounds).is_ok() };
        let old = (bounds.left, bounds.top, bounds.right, bounds.bottom);
        if minimized {
            // 预防实验：最小化时把视口拉回还原尺寸，避免 controller 停留在 160x28——
            // 还原路径的卡死疑似与"缩小到 160x28 再放大"的缩放竞态有关
            bounds.left = 0;
            bounds.top = 0;
            bounds.right = LAST_VIEW_W.load(std::sync::atomic::Ordering::SeqCst);
            bounds.bottom = LAST_VIEW_H.load(std::sync::atomic::Ordering::SeqCst);
            let s_ok = unsafe { controller.SetBounds(bounds).is_ok() };
            out.push_str(&format!(
                "Bounds(ok={}) old=({},{},{},{}); SetBounds-back-to({},{})={}",
                b_ok, old.0, old.1, old.2, old.3, bounds.right, bounds.bottom, s_ok
            ));
        } else {
            bounds.left = 0;
            bounds.top = 0;
            bounds.right = right;
            bounds.bottom = bottom;
            let s_ok = unsafe { controller.SetBounds(bounds).is_ok() };
            out.push_str(&format!(
                "Bounds(ok={}) old=({},{},{},{}); SetBounds(0,0,{},{})={}",
                b_ok, old.0, old.1, old.2, old.3, right, bottom, s_ok
            ));
        }
        log_fix(format!(
            "FIX minimized={} {} {} -> {}",
            minimized, state, hwnd_state, out
        ));
    });
    if let Err(e) = r {
        log_fix(format!("FIX with_webview ERROR {:?}", e));
    }
}

fn main() {
    log_fix(format!(
        "APP START pid={} logfile={}",
        std::process::id(),
        std::env::temp_dir().join("tinux-webview-fix.log").display()
    ));
    // 心跳线程：UI 线程卡死时它仍会继续写日志，配合事件日志可区分"进程死了"vs"UI 线程卡死"；
    // iconic 反映 OS 层窗口状态，能判断用户点任务栏还原时 OS 到底还原了没有
    std::thread::spawn(|| {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let h = MAIN_HWND.load(std::sync::atomic::Ordering::SeqCst) as isize;
            if h == 0 {
                log_fix("HB tick hwnd=0");
            } else {
                let hwnd = HWND(h);
                unsafe {
                    log_fix(format!(
                        "HB tick iconic={} vis={}",
                        IsIconic(hwnd).as_bool(),
                        IsWindowVisible(hwnd).as_bool()
                    ));
                }
            }
        }
    });
    tauri::Builder::default()
        .on_window_event(|event| {
            let window = event.window();
            // 记录 HWND 供心跳线程用
            if let Ok(h) = window.hwnd() {
                MAIN_HWND.store(h.0 as usize, std::sync::atomic::Ordering::SeqCst);
            }
            // WndProc 观察钩子（只装一次）：捕获任务栏还原的消息流
            install_wndproc_hook(window);
            // 诊断版：所有事件 + 窗口状态逐条落盘（%TEMP%\tinux-webview-fix.log）
            log_fix(format!(
                "EVENT {:?} | {} | {}",
                event.event(),
                window_state_str(window),
                hwnd_state_str(window)
            ));
            // 只认 Resized：日志已证明还原必然触发 Resized，不再用 Focused 兜底，
            // 减少每次点击都 SetBounds 的 COM 干扰
            if let tauri::WindowEvent::Resized(_) = event.event() {
                fix_webview_after_minimize_restore(window);
            }
        })
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
        .expect("error while running tinux");
}
