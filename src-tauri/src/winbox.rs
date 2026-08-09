// WinBox 集成模块
use std::process::Command;
use std::path::PathBuf;
use std::io::{Read, Write};
use std::sync::Mutex;
use std::collections::HashMap;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::Manager;

// 常驻交互会话（ConPTY）
struct ShellSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

// 会话输出事件负载（前端按 session_id 路由）
#[derive(serde::Serialize, Clone)]
struct ShellOutput {
    session_id: String,
    text: String,
}

// 会话退出事件负载
#[derive(serde::Serialize, Clone)]
struct ShellExit {
    session_id: String,
}

// 输出解码：优先 UTF-8（busybox/node 等原生输出）；
// 非法 UTF-8 时按 GBK 解码（中文 Windows 下 ping/ipconfig 等系统命令输出为 GBK）
fn decode_output(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::GBK.decode(bytes);
            decoded.into_owned()
        }
    }
}

pub struct WinBox {
    root: PathBuf,
    sessions: Mutex<HashMap<String, ShellSession>>,
}

impl WinBox {
    pub fn new(root: PathBuf) -> Self {
        WinBox {
            root,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    // 会话表锁（容忍中毒）
    fn sessions_mut(&self) -> std::sync::MutexGuard<'_, HashMap<String, ShellSession>> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---------- 常驻交互 shell（ConPTY） ----------

    // 启动常驻 busybox sh（ConPTY 伪终端），输出经 Tauri 事件流式推送
    pub fn shell_start(&self, session_id: &str, app: tauri::AppHandle, rows: u16, cols: u16) -> Result<(), String> {
        // 同名旧会话先停掉（panel 复用场景）
        self.shell_stop(session_id);
        let sid = session_id.to_string();

        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let busybox = self.root.join("bin").join("busybox.exe");
        let mut cmd = CommandBuilder::new(busybox.to_string_lossy().into_owned());
        cmd.arg("sh");

        // 环境：模拟 winbox.bat + 伪装层 + 终端能力
        let bin_dir = self.root.join("bin").to_string_lossy().replace('\\', "/");
        let node_dir = self.root.join("bin").join("nodejs").to_string_lossy().replace('\\', "/");
        let app_dir = self.root.join("app").to_string_lossy().replace('\\', "/");
        let local_bin = self.root.join("app").join(".local").join("bin").to_string_lossy().replace('\\', "/");
        let shim = self.root.join("usr").join("lib").join("minilinux.sh");
        cmd.env(
            "PATH",
            format!(
                "{};{};{};{}",
                bin_dir,
                node_dir,
                local_bin,
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        cmd.env("HOME", &app_dir);
        cmd.env("USERPROFILE", &app_dir);
        cmd.env("TERM", "xterm-256color");
        // uv 工具目录：~/.local/bin（uv tool install 默认落点）已在 PATH 中；
        // 并让 uv 以后直接把工具装进 winbox/bin
        cmd.env("UV_TOOL_BIN_DIR", &bin_dir);
        // TMPDIR：busybox-w32 无 /tmp，安装器 mktemp 需要可写临时目录
        let tmp_dir = self.root.join("app").join("tmp");
        let _ = std::fs::create_dir_all(&tmp_dir);
        cmd.env("TMPDIR", tmp_dir.to_string_lossy().replace('\\', "/"));
        // ENV：busybox ash 交互模式启动时自动 source 伪装层（uname → Linux、PS1）
        cmd.env("ENV", shim.to_string_lossy().replace('\\', "/"));
        cmd.cwd(self.root.join("app"));

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        // 输出事件线程：逐块读取并推送到前端（payload 带 session_id）
        let app2 = app.clone();
        let sid2 = sid.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let payload = ShellOutput {
                            session_id: sid2.clone(),
                            text: decode_output(&buf[..n]),
                        };
                        let _ = app2.emit_all("shell-output", payload);
                    }
                }
            }
            // 会话结束通知前端
            let _ = app2.emit_all("shell-exit", ShellExit { session_id: sid2 });
        });

        self.sessions_mut().insert(sid, ShellSession {
            master: pair.master,
            writer,
            child,
        });
        Ok(())
    }

    // 写入输入（原始字节：回车、控制序列、UTF-8 文本）
    pub fn shell_write(&self, session_id: &str, data: &str) -> Result<(), String> {
        if let Some(s) = self.sessions_mut().get_mut(session_id) {
            s.writer
                .write_all(data.as_bytes())
                .map_err(|e| e.to_string())?;
            s.writer.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    // 调整终端尺寸（全屏 TUI 重排）
    pub fn shell_resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        if let Some(s) = self.sessions_mut().get_mut(session_id) {
            s.master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    // 停止指定会话
    pub fn shell_stop(&self, session_id: &str) {
        let removed = self.sessions_mut().remove(session_id);
        if let Some(mut s) = removed {
            // ConPTY 释放可能阻塞（残留子进程仍持有控制台句柄时 close 会挂起）。
            // 同步命令跑在主线程上，必须把 kill + drop 挪到后台线程，否则整个界面卡死。
            std::thread::spawn(move || {
                let _ = s.child.kill();
                drop(s);
            });
        }
    }

    // 列出活跃会话 id
    pub fn shell_list(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sessions_mut().keys().cloned().collect();
        ids.sort();
        ids
    }

    // 执行 WinBox 命令
    pub fn run(&self, command: &str) -> Result<String, String> {
        let busybox = self.root.join("bin").join("busybox.exe");
        // mini-linux 伪装层：每条命令执行前先 source 补丁脚本（如 uname → Linux）
        let shim = self.root.join("usr").join("lib").join("minilinux.sh");
        let shim_path = shim.to_string_lossy().replace('\\', "/");
        let shim_arg = format!(
            "[ -f \"{}\" ] && . \"{}\"; {}",
            shim_path, shim_path, command
        );
        // 模拟 winbox.bat：PATH 前置 bin 目录，HOME/USERPROFILE 指向 app
        let bin_dir = self.root.join("bin").to_string_lossy().replace('\\', "/");
        let node_dir = self.root.join("bin").join("nodejs").to_string_lossy().replace('\\', "/");
        let app_dir = self.root.join("app").to_string_lossy().replace('\\', "/");
        let path = format!(
            "{};{};{}",
            bin_dir,
            node_dir,
            std::env::var("PATH").unwrap_or_default()
        );

        let output = if busybox.exists() {
            // 使用 busybox sh 执行
            Command::new(&busybox)
                .env("PATH", &path)
                .env("HOME", &app_dir)
                .env("USERPROFILE", &app_dir)
                .args(["sh", "-c", &shim_arg])
                .current_dir(self.root.join("app"))
                .output()
        } else {
            // 回退到系统 sh
            Command::new("sh")
                .env("PATH", &path)
                .env("HOME", &app_dir)
                .env("USERPROFILE", &app_dir)
                .args(["-c", &shim_arg])
                .current_dir(self.root.join("app"))
                .output()
        };

        match output {
            Ok(output) => {
                let stdout = decode_output(&output.stdout);
                let stderr = decode_output(&output.stderr);
                if output.status.success() {
                    Ok(stdout)
                } else {
                    Err(stderr)
                }
            }
            Err(e) => Err(e.to_string()),
        }
    }
}
