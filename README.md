# feier-three 🥰 飞儿一体 v3

> **透明 AI 终端 · 在 Windows 上假装自己是 Linux**
>
> A transparent, multi-panel AI terminal that turns Windows into a *mini-Linux* — no virtualization, no WSL, instant start.

---

## 🎯 它是什么

feier-three 是一个基于 **Tauri + ConPTY + xterm.js** 的终端应用。它的核心哲学：

**本质是给 WinBox（一个免虚拟化的 mini-Linux 环境）加一个漂亮的可视化窗口。**

一个窗口内可以开**多个 tab、多个分屏 panel**，每个 panel 都是独立的 Linux 风格 shell（busybox），并且能直接运行 kimi、openclaw、MiMo Code、hermes、prime-agent 等主流 AI agent。

## ✨ 特性亮点

| 特性 | 说明 |
|------|------|
| 🖥️ **真终端，不是假管道** | 后端用 **Windows ConPTY**（伪终端）+ 前端 **xterm.js** —— 全屏 TUI 应用（kimi / openclaw）原生渲染、可交互，不是"一次性捕获输出" |
| 📑 **多 tab + 多分屏** | 浏览器式标签页 + 二分分割布局（左右/上下），每个 panel 独立 shell 会话，**切换不杀进程** |
| 🐧 **mini-Linux 伪装层** | busybox 180+ 命令、`uname` 伪装成 Linux（脚本 OS 检测全绿）、PS1 定制、`/tmp` 修复 |
| 🤖 **AI agent 实测兼容** | 全部走 npm/pip/uv 安装并在本终端跑通：**kimi-code、@mimo-ai/cli (MiMo Code)、openclaw、prime-agent、hermes-agent** |
| 🛡️ **安装脚本智能检查** | `curl \| sh` 拦截器：检测到"纯二进制分发"（无 pip/uv/npm 渠道）自动警告中止，避免下载必然跑不了的 Linux ELF |
| 🪟 **透明窗口 + 设置面板** | 整窗透明度、字体、字号、前景/背景/光标色，localStorage 持久化 |
| 🎬 **内置视频播放** | `video <URL>` 命令直接弹播放器（busybox httpd 做本地文件服务） |
| 🔤 **中文 IME 支持** | 输入法合成提交的渲染残影已做缓解 |
| ⚡ **零依赖、秒启动** | 无虚拟化、无 Docker、无系统级安装 |

## 📸 截图

![界面截图 1](screenshots/111.png)

![界面截图 2](screenshots/222.png)

## 🏗️ 技术架构

```
┌─────────────────────────────────────────────────────────┐
│ 前端 (WebView2 / 无打包器)                                │
│   xterm.js × N (每个 panel 一个实例)                      │
│   tab 布局树 + 分屏引擎 + 设置面板 + 标记桥                │
└──────────────┬──────────────────────────────────────────┘
               │ Tauri 事件流 (shell-output / shell-exit, 按 session_id 路由)
               │ invoke: shell_start / shell_write / shell_resize / shell_stop
┌──────────────▼──────────────────────────────────────────┐
│ 后端 (Rust, src-tauri)                                   │
│   WinBox.sessions: HashMap<session_id, ConPTY 会话>       │
│   portable-pty (Windows ConPTY) + busybox sh 常驻         │
│   PATH/HOME/USERPROFILE/TMPDIR/ENV 注入                   │
└──────────────┬──────────────────────────────────────────┘
               │ CreateProcess / 管道
┌──────────────▼──────────────────────────────────────────┐
│ 运行时 (winbox/, 本仓库不含，见"构建运行时")               │
│   busybox64u + Python(embeddable) + Node.js/npm + uv      │
└─────────────────────────────────────────────────────────┘
```

- **进程层**：[src-tauri/src/winbox.rs](src-tauri/src/winbox.rs) — ConPTY 会话表（多会话并存）、环境注入、输出事件流
- **通信层**：[src-tauri/src/main.rs](src-tauri/src/main.rs) — shell 系列命令 + `run_command`（兼容保留）
- **界面层**：[src/scripts/terminal.js](src/scripts/terminal.js) — 多 panel/tab、事件路由、设置、视频、安装检查桥
- **伪装层**：[winbox/usr/lib/minilinux.sh](winbox/usr/lib/minilinux.sh) — uname 伪装、PS1、TMPDIR、`sh`/`bash` 安装检查拦截、video/help 标记桥

## 🚀 快速开始

### 1. 准备构建环境（Windows）

| 依赖 | 说明 |
|------|------|
| Rust | GNU 工具链：`rustup default stable-x86_64-pc-windows-gnu` |
| MSYS2 | 提供 mingw-w64 binutils（dlltool/windres/gcc），装于 `C:\msys64` |
| Node.js | 任意现代版本（仅用于 tauri CLI） |

### 2. 安装依赖 + 一键初始化运行时

```bash
npm install
init.bat        # 一键组装 winbox 运行时（下载 busybox/python/node + 安装伪装层）
start.bat       # 启动开发环境（= set PATH=C:\msys64\mingw64\bin;%PATH% && npm run tauri dev）
```

`init.bat` 自动下载并组装 `winbox/` 运行时（busybox64u + Python embeddable + Node.js，全部便携免安装），并把 [runtime/minilinux.sh](runtime/minilinux.sh) 伪装层安装到 `winbox/usr/lib/`；已存在运行时则跳过下载。

> 直接从 v2 拷贝 `winbox/` 目录亦可。

## ⌨️ 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+Shift+T` | 新建 tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 切换 tab |
| `Ctrl+Shift+Q` | 关闭当前 tab |
| `Ctrl+Shift+\` | 左右分屏 |
| `Ctrl+Shift+-` | 上下分屏 |
| `Ctrl+Shift+[` / `]` | 切换 panel 焦点 |
| `Ctrl+Shift+W` | 关闭当前 panel（tab 只剩一个时关 tab） |
| `Esc` | 关闭视频播放器 / 设置面板 |

## 📁 目录结构

```
feier-three/
├── src/                    # 前端（无打包器，直接静态伺服）
│   ├── index.html
│   ├── styles/terminal.css
│   ├── scripts/terminal.js
│   └── vendor/             # xterm.js UMD（自带，无 CDN 依赖）
├── src-tauri/              # Rust 后端
│   ├── src/main.rs         # Tauri 命令注册
│   ├── src/winbox.rs       # ConPTY 会话表 + 环境注入
│   └── tauri.conf.json
├── winbox/                 # 运行时（init.bat 自动组装，不入仓库）
├── runtime/minilinux.sh    # mini-Linux 伪装层模板（init.bat 安装到 winbox/usr/lib/）
├── init.bat                # 一键组装运行时（下载 busybox/python/node）
├── start.bat               # 启动开发环境（注入 MSYS2 binutils PATH）
└── package.json
```

## 🤖 已实测兼容的 AI Agent

| Agent | 安装命令 | 验证 |
|-------|---------|------|
| kimi-code (Moonshot) | `npm install -g @moonshot-ai/kimi-code` | ✅ TUI 运行 |
| MiMo Code (小米) | `npm install -g @mimo-ai/cli` | ✅ CLI 运行 |
| openclaw | `npm install -g openclaw` | ✅ 全屏 TUI |
| prime-agent (Prime Intellect) | 官方 install.sh（内部即 npm tarball） | ✅ 含 uv/IPython 引导 |
| hermes-agent (Nous Research) | `uv tool install hermes-agent` | ✅ Python 生态 |

## ⚠️ 已知限制（诚实声明）

1. **Linux ELF 二进制无法执行** —— 这是 Windows 加载器的硬边界。伪装层能骗过 `uname` 检测，但骗不过进程加载器。纯二进制分发的安装器会被内置检查拦截。
2. **中文 IME 宽字符** —— xterm 5.3 合成渲染有已知上游问题（[#6060](https://github.com/xtermjs/xterm.js/pull/6060) 未合并），已做缓解。
3. **`curl \| sh` 检查是启发式** —— 防呆提示，非安全围栏。
4. 开发期重建需先关闭运行中的实例（Windows 文件锁，os error 32）。

## 🗺️ 路线图

- **v1** feier-one：Tauri + 单命令捕获终端（冻结）
- **v2** feier-two：ConPTY + xterm.js 真终端（冻结）
- **v3** feier-three：多 tab + 多 panel + 设置面板 + 安装检查（当前）
- **未来**：多 panel 拖拽、会话持久化/重连、发行版打包（内置 PortableGit）、agent 命令桥

## 📄 License

MIT

---

*Created by 爸爸 & 飞儿 🥰*
