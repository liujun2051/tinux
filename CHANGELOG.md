# tinux 修复日志

## v0.2.8（2026-08-13）
- **half clear 根治**：tab 创建于窗口最小化/隐藏或字体度量未完成时，xterm 渲染
  服务字符尺寸（cell）为 0，fit addon 静默失败、term 永久停在默认 24x80
  （stty 可验证）→ clear 只清半屏且切换不恢复
- robustFit：检测到 cell 为 0 自动延迟重试（250ms × 最多 40 次），fit 后显式
  shell_resize 同步后端；窗口重新激活 / 缩放 / tab 切换时全量重对齐

## v0.2.7（2026-08-13）
- 多 tab 切换不再销毁重建面板 DOM（xterm canvas 常驻），修复随机"clear 只清半屏"
- 分割/关闭面板时才重建对应 tab 的 pane

## v0.2.6（2026-08-13）
- 后端 pending_resize：会话创建前到达的 resize 暂存、创建后补应用（尺寸不再丢失）
- minilinux.sh 的 clear 先重置滚动区域（`\033[r`）再清屏，清除全屏 TUI 残留

## v0.2.5（未发布，NSIS 打包失败跳过）

## v0.2.4（2026-08-13）
- 会话就绪后强制重新 fit + shell_resize，对齐 ConPTY 与前端尺寸

## v0.2.3（2026-08-13）
- python 自包含：bin\python.exe 内置 DLL + python312._pth（不再依赖系统 Python312，
  修复目标机器 0xc0000135 闪退/报错）
- init.bat 同步：重建运行时同样自包含

## v0.2.2（2026-08-13）
- 安装包不内置 WebView2（skip 模式，保持 ~52MB）；目标机器需已装 WebView2 Runtime
- 应用图标换为 vicky.jpg（淡青色，多尺寸）

## v0.2.1（2026-08-13）
- opencode 欢迎界面错行：管线保留裸 LF 行尾（不再改写 CRLF 导致列重置）
- sync 帧（2026）内跨块内容统一进帧缓冲，不再直接渲染（修复错位/字面文本泄漏）

## 新增（v0.2.1 起）
- telnetd 自包含服务（runtime/telnetd.py + telnetd.bat）：ConPTY busybox sh，
  输出批处理防闪烁，无 Windows 服务依赖，winbox 目录拷走即用
