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

// Agent 安装输出事件负载
#[derive(serde::Serialize, Clone)]
struct AgentInstallOutput {
    agent: String,
    text: String,
}

// Agent 安装完成事件负载
#[derive(serde::Serialize, Clone)]
struct AgentInstallDone {
    agent: String,
    code: i32,
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
    // true = 当前系统无法运行 u 版（UTF-8 manifest 仅 Win10 1903+ 支持），
    // 需用 ANSI 版 busybox-ansi.exe（无 UTF-8 manifest，也无 GetACP 检查）
    ansi: bool,
    sessions: Mutex<HashMap<String, ShellSession>>,
}

impl WinBox {
    pub fn new(root: PathBuf) -> Self {
        let ansi = Self::probe_ansi(&root);
        WinBox {
            root,
            ansi,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    // 探测 u 版 busybox 能否在本系统运行：<Win10 1903（如 Server 2016/2019）上
    // u 版启动即打印 "UTF8 manifest not supported" 并退出（libbb/appletlib.c
    // 的 FAIL_IF_UTF8_MANIFEST_UNSUPPORTED 检查）。探测一次，进程生命周期内缓存。
    fn probe_ansi(root: &PathBuf) -> bool {
        let ansi = root.join("bin").join("busybox-ansi.exe");
        if !ansi.exists() {
            return false; // 旧运行时没有 ANSI 版，只能用 u 版
        }
        let u = root.join("bin").join("busybox.exe");
        match std::process::Command::new(&u).args(["sh", "-c", "exit 0"]).output() {
            Ok(o) => String::from_utf8_lossy(&o.stderr).contains("UTF8 manifest"),
            Err(_) => false,
        }
    }

    // 当前系统适用的 busybox 路径
    fn busybox(&self) -> PathBuf {
        if self.ansi {
            self.root.join("bin").join("busybox-ansi.exe")
        } else {
            self.root.join("bin").join("busybox.exe")
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

        let busybox = self.busybox();
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
        // npm 全局目录：强制 winbox 的 npm -g 装进 winbox/bin/nodejs（可移植，已在 PATH）。
        // 否则读用户 .npmrc 的 prefix=d:\node 等系统路径，装完不在 PATH 上（prime-agent 教训）
        cmd.env("npm_config_prefix", &node_dir);
        // npm 缓存也进 winbox（完全自包含，不依赖 Windows 用户目录 %APPDATA%\npm-cache）
        cmd.env("npm_config_cache", format!("{}/.npm-cache", app_dir));
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

    // ---------- Agent 安装中心 ----------

    // 目录下是否有以 prefix 开头的可执行文件（npm shim 是 .cmd，uv 在 Windows 生成 .exe shim，
    // 且工具可能有多个入口如 hermes / hermes-acp / hermes-agent，故按前缀扫描）
    fn has_bin_prefix(dir: &std::path::Path, prefix: &str) -> bool {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if let Some(name) = e.file_name().to_str() {
                    if name.to_ascii_lowercase().starts_with(prefix) {
                        return true;
                    }
                }
            }
        }
        false
    }

    // 检测 agent 是否已安装（npm shim / uv 工具）
    pub fn agent_installed(&self, agent: &str) -> bool {
        let nodejs = self.root.join("bin").join("nodejs");
        let local_bin = self.root.join("app").join(".local").join("bin");
        let bin = self.root.join("bin");
        match agent {
            "claude-code" => Self::has_bin_prefix(&nodejs, "claude"),
            "codex" => Self::has_bin_prefix(&nodejs, "codex"),
            "openclaw" => Self::has_bin_prefix(&nodejs, "openclaw"),
            "opencode" => Self::has_bin_prefix(&nodejs, "opencode"),
            "kimi-code" => Self::has_bin_prefix(&nodejs, "kimi"),
            "hermes" => Self::has_bin_prefix(&local_bin, "hermes") || Self::has_bin_prefix(&bin, "hermes"),
            "antigravity" => Self::has_bin_prefix(&nodejs, "antigravity"),
            "gemini-cli" => Self::has_bin_prefix(&nodejs, "gemini"),
            "grok-build" => Self::has_bin_prefix(&nodejs, "grok"),
            "cursor" => Self::has_bin_prefix(&nodejs, "cursor"),
            "qwen-code" => Self::has_bin_prefix(&nodejs, "qwen"),
            "qoder" => Self::has_bin_prefix(&nodejs, "qoder"),
            "copilot" => Self::has_bin_prefix(&nodejs, "copilot"),
            "pi" => Self::has_bin_prefix(&nodejs, "pi"),
            "kiro" => Self::has_bin_prefix(&nodejs, "kiro"),
            "kilo" => Self::has_bin_prefix(&nodejs, "kilo"),
            "mistral-vibe" => Self::has_bin_prefix(&local_bin, "mistral-vibe") || Self::has_bin_prefix(&bin, "mistral-vibe"),
            "deepseek-tui" => Self::has_bin_prefix(&nodejs, "deepseek"),
            "reasonix" => Self::has_bin_prefix(&nodejs, "reasonix"),
            "aider" => Self::has_bin_prefix(&local_bin, "aider") || Self::has_bin_prefix(&bin, "aider"),
            "devin" => Self::has_bin_prefix(&nodejs, "devin"),
            "trae" => Self::has_bin_prefix(&nodejs, "trae"),
            _ => false,
        }
    }

    // agent 对应的（程序, 参数）：install=true 安装 / false 卸载
    fn agent_cmd(&self, agent: &str, install: bool) -> Result<(std::path::PathBuf, Vec<&'static str>), String> {
        let nodejs = self.root.join("bin").join("nodejs");
        let npm = nodejs.join("npm.cmd");
        let uv = self.root.join("bin").join("uv.exe");
        let (prog, args): (std::path::PathBuf, Vec<&'static str>) = match (agent, install) {
            ("claude-code", true) => (npm.clone(), vec!["install", "-g", "@anthropic-ai/claude-code@latest"]),
            ("claude-code", false) => (npm.clone(), vec!["uninstall", "-g", "@anthropic-ai/claude-code"]),
            ("codex", true) => (npm.clone(), vec!["install", "-g", "@openai/codex@latest"]),
            ("codex", false) => (npm.clone(), vec!["uninstall", "-g", "@openai/codex"]),
            ("openclaw", true) => (npm.clone(), vec!["install", "-g", "openclaw@latest"]),
            ("openclaw", false) => (npm.clone(), vec!["uninstall", "-g", "openclaw"]),
            ("opencode", true) => (npm.clone(), vec!["install", "-g", "opencode-ai@latest"]),
            ("opencode", false) => (npm.clone(), vec!["uninstall", "-g", "opencode-ai"]),
            ("kimi-code", true) => (npm.clone(), vec!["install", "-g", "@moonshot-ai/kimi-code@latest"]),
            ("kimi-code", false) => (npm.clone(), vec!["uninstall", "-g", "@moonshot-ai/kimi-code"]),
            ("hermes", true) => (uv.clone(), vec!["tool", "install", "hermes-agent"]),
            ("hermes", false) => (uv.clone(), vec!["tool", "uninstall", "hermes-agent"]),
            // ---- 扩展 agent（2026-08-11 coding_agents_logos；包名经 registry 描述核实） ----
            ("gemini-cli", true) => (npm.clone(), vec!["install", "-g", "@google/gemini-cli@latest"]),
            ("gemini-cli", false) => (npm.clone(), vec!["uninstall", "-g", "@google/gemini-cli"]),
            ("qwen-code", true) => (npm.clone(), vec!["install", "-g", "@qwen-code/qwen-code@latest"]),
            ("qwen-code", false) => (npm.clone(), vec!["uninstall", "-g", "@qwen-code/qwen-code"]),
            ("copilot", true) => (npm.clone(), vec!["install", "-g", "@github/copilot@latest"]),
            ("copilot", false) => (npm.clone(), vec!["uninstall", "-g", "@github/copilot"]),
            ("aider", true) => (uv.clone(), vec!["tool", "install", "aider-chat"]),
            ("aider", false) => (uv.clone(), vec!["tool", "uninstall", "aider-chat"]),
            ("mistral-vibe", true) => (uv.clone(), vec!["tool", "install", "mistral-vibe"]),
            ("mistral-vibe", false) => (uv.clone(), vec!["tool", "uninstall", "mistral-vibe"]),
            // 包名未核实（antigravity/cursor/grok-build/qoder/pi/kilo/kiro/deepseek-tui/reasonix/devin/trae）：
            // 同名 npm 包可能是抢注/无关项目（antigravity=placeholder、devin=个人名、trae=HTTP 客户端），
            // 安装命令待补，避免装错
            ("antigravity", _) | ("cursor", _) | ("grok-build", _) | ("qoder", _) | ("pi", _) | ("kilo", _)
            | ("kiro", _) | ("deepseek-tui", _) | ("reasonix", _) | ("devin", _) | ("trae", _) => {
                return Err(format!("install command not configured for {}", agent))
            }
            _ => return Err(format!("unknown agent: {}", agent)),
        };
        Ok((prog, args))
    }

    // 安装 agent（npm/uv 生态，走 winbox 环境；输出经事件流式推送）
    pub fn agent_install(&self, app: tauri::AppHandle, agent: &str) -> Result<(), String> {
        let (prog, args) = self.agent_cmd(agent, true)?;
        self.spawn_agent(app, agent, prog, args)
    }

    // 卸载 agent
    pub fn agent_uninstall(&self, app: tauri::AppHandle, agent: &str) -> Result<(), String> {
        let (prog, args) = self.agent_cmd(agent, false)?;
        self.spawn_agent(app, agent, prog, args)
    }

    // 实际执行：spawn 子进程 + stdout/stderr 双线程流式推送 + 后台等待退出
    fn spawn_agent(&self, app: tauri::AppHandle, agent: &str, prog: std::path::PathBuf, args: Vec<&'static str>) -> Result<(), String> {
        if !prog.exists() {
            return Err(format!("program not found: {}", prog.display()));
        }

        let bin_dir = self.root.join("bin").to_string_lossy().replace('\\', "/");
        let node_dir = self.root.join("bin").join("nodejs").to_string_lossy().replace('\\', "/");
        let app_dir = self.root.join("app").to_string_lossy().replace('\\', "/");
        let local_bin = self.root.join("app").join(".local").join("bin").to_string_lossy().replace('\\', "/");
        let tmp_dir = self.root.join("app").join("tmp");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let path = format!(
            "{};{};{};{}",
            bin_dir,
            node_dir,
            local_bin,
            std::env::var("PATH").unwrap_or_default()
        );

        let mut child = std::process::Command::new(&prog)
            .args(&args)
            .env("PATH", &path)
            .env("HOME", &app_dir)
            .env("USERPROFILE", &app_dir)
            .env("TMPDIR", tmp_dir.to_string_lossy().replace('\\', "/"))
            .env("UV_TOOL_BIN_DIR", &bin_dir)
            .env("npm_config_prefix", &node_dir)
            .env("npm_config_cache", format!("{}/.npm-cache", app_dir))
            .current_dir(self.root.join("app"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;

        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let a1 = agent.to_string();
        let a2 = agent.to_string();
        let a3 = agent.to_string();
        let app2 = app.clone();
        let app3 = app.clone();

        // stdout / stderr 各一个读取线程，逐块推送
        std::thread::spawn(move || {
            use std::io::Read;
            let mut r = stdout;
            let mut buf = [0u8; 4096];
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let _ = app2.emit_all(
                            "agent-install-output",
                            AgentInstallOutput {
                                agent: a1.clone(),
                                text: decode_output(&buf[..n]),
                            },
                        );
                    }
                }
            }
        });
        std::thread::spawn(move || {
            use std::io::Read;
            let mut r = stderr;
            let mut buf = [0u8; 4096];
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let _ = app3.emit_all(
                            "agent-install-output",
                            AgentInstallOutput {
                                agent: a2.clone(),
                                text: decode_output(&buf[..n]),
                            },
                        );
                    }
                }
            }
        });

        // 等待退出 → 完成事件
        let app4 = app;
        std::thread::spawn(move || {
            let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            let _ = app4.emit_all("agent-install-done", AgentInstallDone { agent: a3, code });
        });

        Ok(())
    }

    // 执行 WinBox 命令
    pub fn run(&self, command: &str) -> Result<String, String> {
        let busybox = self.busybox();
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
                .env("npm_config_prefix", &node_dir)
                .env("npm_config_cache", format!("{}/.npm-cache", app_dir))
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
