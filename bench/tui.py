#!/usr/bin/env python3
"""Run pepe's dashboard headlessly, in a pseudo-terminal, and measure it.

    bench/tui.py [--cols 160 --rows 48] -- PEPE_BINARY ARGS...

The dashboard needs a terminal, so one is faked here. Frames are read and
discarded as pepe draws them (a terminal that stops reading would block the
dashboard). Once pepe reports the run finished, or after --timeout seconds,
`q` is sent. Prints the process's wall, user and system CPU time and its peak
memory, like bench/measure.sh.
"""
import argparse
import fcntl
import os
import pty
import re
import resource
import select
import struct
import subprocess
import sys
import termios
import time

ESCAPES = re.compile(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07|\x1b[()][A-Za-z0-9]")
# What the dashboard's title says once the run is over
OVER = re.compile(rb"done|stopped|Healthy|Degraded|Failing")


def cpu_seconds(pid):
    """CPU time of a live process, from ps (whole seconds)"""
    try:
        out = subprocess.run(["ps", "-o", "cputime=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
        h, m, s = ([0, 0] + [float(x) for x in out.split(":")])[-3:]
        return h * 3600 + m * 60 + s
    except Exception:
        return None


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--cols", type=int, default=160)
    p.add_argument("--rows", type=int, default=48)
    p.add_argument("--timeout", type=float, default=600)
    p.add_argument("--keys", default="", help="keys to send 1s after start, e.g. '3' for the Requests tab")
    p.add_argument("--settle", type=float, default=0.5, help="seconds between the run finishing and q")
    p.add_argument("--show", action="store_true", help="also print the report pepe leaves in the shell")
    p.add_argument("cmd", nargs=argparse.REMAINDER)
    a = p.parse_args()
    cmd = a.cmd[1:] if a.cmd and a.cmd[0] == "--" else a.cmd

    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.environ["PEPE_NO_UPDATE_CHECK"] = "1"
        os.execvp(cmd[0], cmd)

    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", a.rows, a.cols, 0, 0))
    start = time.monotonic()
    frames = 0
    out = bytearray()
    sent_keys = False
    quit_at = None
    # Text of the last frames (escape sequences stripped), to spot the end
    # of the run; and the child's CPU time, as a fallback: a finished
    # dashboard redraws only on input, so its CPU stops climbing
    tail = b""
    last_cpu, idle_since, next_cpu_check = None, None, start + 2.0
    while True:
        now = time.monotonic()
        if not sent_keys and a.keys and now - start > 1.0:
            os.write(fd, a.keys.encode())
            sent_keys = True
        if now >= next_cpu_check:
            next_cpu_check = now + 0.5
            cpu = cpu_seconds(pid)
            if cpu is not None and cpu == last_cpu:
                idle_since = idle_since or now
            else:
                idle_since = None
            last_cpu = cpu
        over = (now - start > 1.0 and OVER.search(ESCAPES.sub(b"", tail))) or \
            (idle_since is not None and now - idle_since >= 3.0) or now - start > a.timeout
        if quit_at is None and over:
            quit_at = now + a.settle
        if quit_at is not None and now >= quit_at:
            os.write(fd, b"q")
            quit_at = float("inf")
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                chunk = os.read(fd, 1 << 16)
            except OSError:
                break
            if not chunk:
                break
            frames += chunk.count(b"\x1b[H")
            tail = (tail + chunk)[-65536:]
        try:
            done, status = os.waitpid(pid, os.WNOHANG)
            if done:
                break
        except ChildProcessError:
            break
    try:
        os.waitpid(pid, 0)
    except ChildProcessError:
        pass
    wall = time.monotonic() - start
    ru = resource.getrusage(resource.RUSAGE_CHILDREN)
    rss_mb = ru.ru_maxrss / 1048576 if sys.platform == "darwin" else ru.ru_maxrss / 1024
    print(f"wall_s={wall:.2f} user_s={ru.ru_utime:.3f} sys_s={ru.ru_stime:.3f} "
          f"cpu_s={ru.ru_utime + ru.ru_stime:.3f} peak_rss_mb={rss_mb:.1f} frames~{frames}")
    if a.show:
        text = ESCAPES.sub(b"", tail).decode("utf-8", "replace")
        lines = [l.rstrip() for l in text.replace("\r", "").split("\n") if l.strip()]
        print("\n".join("    " + l for l in lines[-8:]))


if __name__ == "__main__":
    main()
