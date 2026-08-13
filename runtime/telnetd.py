#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
tinux telnet 服务（自包含守护进程，不依赖 Windows 服务 / SCM）。

数据通路：telnet 客户端 ⇄ 本脚本（IAC 协商 / NAWS 改尺寸 / 换行翻译）
                     ⇄ pywinpty ConPTY ⇄ winbox busybox sh（含 minilinux.sh 伪装层）

自包含原则（与 winbox 一致）：
  - 用 winbox 自带的 python（winbox/bin/python/）运行，winpty 包已内置同目录
  - 整个 winbox 目录拷贝到任何 Windows 机器即用，无注册表 / 服务 / 管理员依赖
    （防火墙放行端口是一次性的管理员操作，仅网络可达需要）

用法：
  python telnetd.py [port]        # 前台运行（默认 23；测试用如 2323）
  python telnetd.py --daemon [port]  # 以守护进程方式后台运行（pythonw 无窗口）
  python telnetd.py --stop        # 按 pidfile 停止守护进程
  python telnetd.py --status      # 查看运行状态

环境变量：
  WINBOX_ROOT   winbox 根目录（默认 = 脚本所在目录的上级，即 winbox/）
  PORT         监听端口（默认 23）
"""
import os
import signal
import socket
import subprocess
import sys
import threading
import time

ROOT = os.environ.get("WINBOX_ROOT") or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOG_FILE = os.path.join(ROOT, "app", "telnetd.log")
PID_FILE = os.path.join(ROOT, "app", "telnetd.pid")

def log(msg):
    line = time.strftime("%Y-%m-%d %H:%M:%S ") + str(msg)
    try:
        os.makedirs(os.path.dirname(LOG_FILE), exist_ok=True)
        with open(LOG_FILE, "a", encoding="utf-8") as f:
            f.write(line + "\n")
    except Exception:
        pass
    if sys.stdout and sys.stdout.isatty():
        print(line)

# ---------------- winbox 环境（复刻 winbox.rs shell_start） ----------------
def winbox_env():
    root = ROOT.replace("\\", "/")
    bin_dir = root + "/bin"
    node_dir = bin_dir + "/nodejs"
    app_dir = root + "/app"
    local_bin = app_dir + "/.local/bin"
    env = {
        "PATH": ";".join([bin_dir, node_dir, local_bin, os.environ.get("PATH", "")]),
        "HOME": app_dir,
        "USERPROFILE": app_dir,
        "TERM": "xterm-256color",
        "UV_TOOL_BIN_DIR": bin_dir,
        "npm_config_prefix": node_dir,
        "npm_config_cache": app_dir + "/.npm-cache",
        "TMPDIR": app_dir + "/tmp",
        "TEMP": os.environ.get("TEMP", r"C:\Windows\Temp"),
        "TMP": os.environ.get("TMP", r"C:\Windows\Temp"),
        "ENV": root + "/usr/lib/minilinux.sh",
    }
    return env

def env_string():
    return "\0".join(f"{k}={v}" for k, v in winbox_env().items()) + "\0"

# ---------------- telnet 协议（最小 IAC 协商） ----------------
IAC, DONT, DO, WONT, WILL, SB, SE = 255, 254, 253, 252, 251, 250, 240
ECHO, SGA, BINARY, NAWS = 1, 3, 0, 31

def reply_to_do(opt):
    return WILL if opt in (BINARY, SGA, NAWS) else WONT

def reply_to_will(opt):
    return DO if opt in (BINARY, SGA) else DONT


class TelnetSession(threading.Thread):
    """一个 telnet 连接 = 一个 ConPTY busybox sh 会话"""

    def __init__(self, sock, addr, cols=80, rows=24):
        super().__init__(daemon=True)
        self.sock = sock
        self.addr = addr
        self.cols, self.rows = cols, rows
        self.pty = None

    def run(self):
        from winpty import PTY  # 延迟导入：--stop/--status 无需 winpty

        log(f"session start {self.addr[0]}:{self.addr[1]}")
        try:
            self.pty = PTY(self.cols, self.rows)
            self.pty.spawn(os.path.join(ROOT, "bin", "busybox.exe"),
                           cmdline="sh",
                           cwd=os.path.join(ROOT, "app"),
                           env=env_string())
            self.sock.settimeout(0.05)
            self.loop()
        except Exception as e:
            log(f"session error {self.addr}: {e!r}")
        finally:
            try:
                self.pty.close()
            except Exception:
                pass
            try:
                self.sock.close()
            except Exception:
                pass
            log(f"session end {self.addr[0]}:{self.addr[1]}")

    def loop(self):
        sockbuf = b""
        outbuf = b""   # pty -> 客户端 输出批处理缓冲
        last_out = 0.0
        try:
            self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        except OSError:
            pass
        while True:
            try:
                chunk = self.sock.recv(4096)
            except socket.timeout:
                chunk = b""  # 超时：无输入，继续轮询 pty（不能当 EOF！）
                timed_out = True
            except OSError as e:
                log(f"recv OSError: {e!r}")
                break
            if chunk == b"" and not timed_out:
                log("recv EOF (client closed)")
                break
            timed_out = False
            if chunk:
                sockbuf += chunk
                data, sockbuf = self.process_client(sockbuf)
                if data:
                    try:
                        self.pty.write(data.decode("utf-8", "replace"))
                    except Exception as e:
                        log(f"pty write err: {e!r}")
                        break
            try:
                out = self.pty.read(blocking=False)
            except Exception as e:
                log(f"pty read err: {e!r}")
                break
            if out:
                if isinstance(out, str):
                    out = out.encode("utf-8", "replace")
                outbuf += out.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
                last_out = time.time()
            # 批处理：busybox 行编辑器逐字符输出（\x1b[1;9H + char），单独发会
            # 产生大量网络小包导致客户端闪烁/跳跃。空闲 20ms 或攒够 8KB 再发送。
            if outbuf and (time.time() - last_out >= 0.02 or len(outbuf) >= 8192):
                try:
                    self.sock.sendall(outbuf)
                except OSError:
                    break
                outbuf = b""
            time.sleep(0.005)
        # 会话结束前把剩余输出发完
        if outbuf:
            try:
                self.sock.sendall(outbuf)
            except OSError:
                pass

    def process_client(self, buf):
        out = bytearray()
        i = 0
        n = len(buf)
        while i < n:
            b = buf[i]
            if b == IAC:
                if i + 1 >= n:
                    break
                nxt = buf[i + 1]
                if nxt in (DO, WILL, DONT, WONT):
                    if i + 2 >= n:
                        break
                    opt = buf[i + 2]
                    if nxt == DO:
                        resp = WILL if reply_to_do(opt) == WILL else WONT
                        self.sock.sendall(bytes([IAC, resp, opt]))
                    elif nxt == WILL:
                        resp = DO if reply_to_will(opt) == DO else DONT
                        self.sock.sendall(bytes([IAC, resp, opt]))
                    elif nxt == DONT:
                        self.sock.sendall(bytes([IAC, WONT, opt]))
                    else:
                        self.sock.sendall(bytes([IAC, DONT, opt]))
                    i += 3
                elif nxt == SB:
                    end = buf.find(bytes([IAC, SE]), i + 2)
                    if end == -1:
                        break
                    self.handle_sb(buf[i + 2:end])
                    i = end + 2
                elif nxt == IAC:
                    out.append(IAC)
                    i += 2
                else:
                    i += 2
            else:
                out.append(b)
                i += 1
        return bytes(out), buf[i:]

    def handle_sb(self, payload):
        if len(payload) >= 5 and payload[0] == NAWS:
            w = (payload[1] << 8) | payload[2]
            h = (payload[3] << 8) | payload[4]
            if 0 < w < 500 and 0 < h < 200:
                self.cols, self.rows = w, h
                try:
                    self.pty.set_size(w, h)
                except Exception as e:
                    log(f"resize err: {e!r}")


class TelnetServer:
    def __init__(self, port=23, host="0.0.0.0"):
        self.port = port
        self.host = host
        self.srv = None
        self.stop_event = threading.Event()

    def serve(self):
        self.srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        # 注意：Windows 上 SO_REUSEADDR 允许两个进程绑同一端口（连接会被劫持），
        # 不要设置——端口占用应直接报错，便于发现重复启动。
        self.srv.bind((self.host, self.port))
        self.srv.listen(8)
        log(f"tinux telnetd listening on {self.host}:{self.port} (winbox: {ROOT})")
        self.srv.settimeout(0.5)
        while not self.stop_event.is_set():
            try:
                sock, addr = self.srv.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            TelnetSession(sock, addr).start()

    def shutdown(self):
        self.stop_event.set()
        try:
            if self.srv:
                self.srv.close()
        except Exception:
            pass


def write_pidfile():
    try:
        os.makedirs(os.path.dirname(PID_FILE), exist_ok=True)
        with open(PID_FILE, "w") as f:
            f.write(str(os.getpid()))
    except Exception as e:
        log(f"pidfile write err: {e!r}")

def remove_pidfile():
    # 只删自己写的 pidfile：启动失败（如端口被占）的实例不得清掉已在运行的实例的
    # pidfile，否则 status/stop 会失去跟踪目标。
    try:
        if os.path.exists(PID_FILE):
            cur = open(PID_FILE).read().strip()
            if cur == str(os.getpid()):
                os.remove(PID_FILE)
    except OSError:
        pass

def run_foreground(port):
    write_pidfile()
    srv = TelnetServer(port)
    try:
        srv.serve()
    except KeyboardInterrupt:
        srv.shutdown()
    finally:
        remove_pidfile()

def daemonize(port):
    exe = sys.executable
    alt = os.path.join(os.path.dirname(exe), "pythonw.exe")
    if os.path.exists(alt):
        exe = alt  # 无控制台窗口
    script = os.path.abspath(__file__)
    flags = 0
    for f in ("DETACHED_PROCESS", "CREATE_NO_WINDOW", "CREATE_NEW_PROCESS_GROUP"):
        flags |= getattr(subprocess, f, 0)
    subprocess.Popen([exe, script, "--serve", str(port)],
                     creationflags=flags,
                     stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL,
                     close_fds=True)
    print(f"telnetd daemon started on port {port} (pidfile: {PID_FILE})")

def stop():
    if not os.path.exists(PID_FILE):
        print("telnetd not running (no pidfile)")
        return
    pid = int(open(PID_FILE).read().strip())
    try:
        os.kill(pid, signal.SIGTERM)
        print(f"stopped telnetd (pid {pid})")
    except OSError as e:
        print(f"stop failed: {e}")
    finally:
        remove_pidfile()

def status():
    if not os.path.exists(PID_FILE):
        print("telnetd: not running")
        return
    pid = int(open(PID_FILE).read().strip())
    try:
        os.kill(pid, 0)  # Windows: signal 0 = 进程存在性检查
        print(f"telnetd: running (pid {pid})")
    except OSError:
        print(f"telnetd: pid {pid} not alive (stale pidfile)")

if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--") or a in ("--daemon", "--stop", "--status", "--serve")]
    if "--stop" in sys.argv:
        stop()
    elif "--status" in sys.argv:
        status()
    elif "--daemon" in sys.argv:
        port = int(args[-1]) if args and args[-1].isdigit() else int(os.environ.get("PORT", "23"))
        daemonize(port)
    elif "--serve" in sys.argv:
        port = int(args[-1]) if args and args[-1].isdigit() else int(os.environ.get("PORT", "23"))
        run_foreground(port)
    else:
        port = int(args[0]) if args else int(os.environ.get("PORT", "23"))
        run_foreground(port)
