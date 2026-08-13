# mini-linux 伪装层 (tinux)
# 由 winbox.rs 注入 ENV 环境变量，busybox ash 交互模式启动时自动 source：
#   - uname → Linux 伪装
#   - PS1 定制提示符
#   - 内置命令桥（video/help）经标记行由前端识别
#   - 安装脚本安全检查（curl ... | sh / bash 拦截）
# 注意：这是 shell 层伪装，Linux 原生 ELF 二进制依然无法在 Windows 上执行。

# 交互提示符
PS1='mini:\w\$ '

# clear：先重置滚动区域再清屏。全屏 TUI（opencode/vi/less 等）异常退出时可能
# 残留 \x1b[1;NNr（滚动区域只占上半屏），导致 clear 只清 region 内。
clear() {
  printf '\033[r\033[2J\033[H'
}

# TMPDIR：busybox-w32 没有 /tmp，安装器（mktemp -d ${TMPDIR:-/tmp}/...）会失败
export TMPDIR="${TMPDIR:-$HOME/tmp}"
mkdir -p "$TMPDIR" 2>/dev/null || true

uname() {
  [ $# -eq 0 ] && set -- -s
  local out="" flag
  for flag in "$@"; do
    case "$flag" in
      -s | --kernel-name)        out="$out Linux" ;;
      -o | --operating-system)   out="$out GNU/Linux" ;;
      -m | --machine | -p | --processor | -i | --hardware-platform)
                                 out="$out x86_64" ;;
      -r | --kernel-release)     out="$out 5.15.0-tinux" ;;
      -n | --nodename)           out="$out tinux" ;;
      -v | --kernel-version)     out="$out #1 SMP PREEMPT_DYNAMIC mini-linux" ;;
      -a | --all)                out="$out Linux tinux 5.15.0-tinux #1 SMP PREEMPT_DYNAMIC mini-linux x86_64 GNU/Linux" ;;
      *)                         out="$out $(command uname "$flag")" ;;
    esac
  done
  echo "${out# }"
}

# ---------- 内置命令桥（标记行由前端 terminal.js 识别） ----------

# video <URL>：前端弹出播放器
video() {
  printf '__FEIER__video:%s\n' "$1"
}

# help：打印帮助文本
help() {
  cat <<'EOF'
tinux 内置命令：
  video <URL>   播放远程视频（前端播放器，Esc 关闭）
  help          显示此帮助
  pwd           显示当前目录（真实路径）
  clear         清屏
  exit          结束 shell 会话（按任意键重启）
其余命令由 busybox sh 处理：ls, cat, grep, sed, awk, python, node, npm ...
EOF
}

# ---------- 安装脚本安全检查（curl ... | sh / bash 拦截） ----------
# 伪 Linux 无法执行 Linux ELF 二进制。当用户执行 "curl ... | sh|bash" 时，
# 读取脚本内容做启发式检查：若疑似"纯二进制分发"（无包管理器渠道）则警告并中止。

feier_install_check() {
  local script="$1"
  local has_pkg=0 has_bin=0

  # 有包管理器渠道 → 安全（npm/pip/uv/pnpm/yarn/cargo/go/brew/apt/dnf）
  printf '%s' "$script" | grep -qE 'npm (install|i|add)|pnpm (add|install)|yarn (add|install)|pip(3)? (install|i)|uv (tool|pip|sync|add)|cargo install|go install|brew install|dnf install|apt(-get)? install' && has_pkg=1

  # 有"uname 平台选择 + 二进制压缩包下载"迹象 → 疑似纯二进制分发
  printf '%s' "$script" | grep -qE 'uname' \
    && printf '%s' "$script" | grep -qE '\.(tar\.gz|tgz|zip)[[:space:]]|curl.*-o|wget.*-O' && has_bin=1

  if [ "$has_pkg" = 0 ] && [ "$has_bin" = 1 ]; then
    printf '\n\033[31mSorry, pure binary not supported ... this is a fake linux.\033[0m\n'
    printf '\033[33m检测到该安装脚本疑似"纯二进制分发"（无 pip/uv/npm 等包管理器渠道）。\033[0m\n'
    printf '\033[33m伪 Linux 无法执行 Linux ELF 二进制，安装后必然无法运行。\033[0m\n'
    printf '\033[33m建议：改用 pip/uv/npm 渠道，或在 WSL / 真 Linux 中安装。\033[0m\n'
    printf '\033[33m若确认要跳过检查强行执行：%s  |  command sh\033[0m\n' "curl -fsSL <URL>"
    return 1
  fi
  return 0
}

# 拦截 sh：仅当 stdin 为管道且无参数时（即 curl ... | sh 模式）做检查
sh() {
  if [ ! -t 0 ] && [ $# -eq 0 ]; then
    local script
    script=$(cat)
    if ! feier_install_check "$script"; then
      return 1
    fi
    printf '%s\n' "$script" | command sh
  else
    command sh "$@"
  fi
}

# 拦截 bash：同上
bash() {
  if [ ! -t 0 ] && [ $# -eq 0 ]; then
    local script
    script=$(cat)
    if ! feier_install_check "$script"; then
      return 1
    fi
    printf '%s\n' "$script" | command bash
  else
    command bash "$@"
  fi
}
