#!/usr/bin/env python3
"""Linux/macOS PTY acceptance check; standard-library only, no product runtime dependency."""
import argparse
import errno
import fcntl
import json
import http.server
import threading
import os
import pty
import re
import select
import signal
import sqlite3
import struct
import tempfile
import termios
import time
from pathlib import Path

ANSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]")

class Terminal:
    def __init__(self, binary, directory, args=(), columns=80):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(directory)
            os.environ["TERM"] = "xterm-256color"
            os.environ.pop("NO_COLOR", None)
            os.execv(binary, [binary, *args])
        self.buffer = b""
        self.transcript = b""
        self.closed = False
        self.resize(columns)

    def resize(self, columns):
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", 32, columns, 0, 0))
        os.kill(self.pid, signal.SIGWINCH)

    def send(self, data, editing=True):
        # Wait for the editor to enter raw mode; control keys sent during report
        # rendering would otherwise be interpreted by the kernel's cooked mode.
        end = time.monotonic() + 5
        while editing and termios.tcgetattr(self.fd)[3] & termios.ICANON:
            if time.monotonic() > end:
                raise AssertionError("editor did not become ready")
            ready, _, _ = select.select([self.fd], [], [], 0.02)
            if ready:
                data_read = os.read(self.fd, 65536)
                self.transcript += data_read
                if b"\x1b[6n" in data_read:
                    os.write(self.fd, b"\x1b[1;1R")
        self.buffer = b""
        os.write(self.fd, data.encode() if isinstance(data, str) else data)

    def expect(self, text, timeout=15):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            plain = ANSI.sub(b"", self.buffer).decode("utf-8", "replace").replace("\r", "")
            if text in plain:
                return plain
            ready, _, _ = select.select([self.fd], [], [], 0.1)
            if ready:
                try:
                    data = os.read(self.fd, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not data:
                    break
                self.buffer += data
                self.transcript += data
                # Answer a terminal cursor-position query if the editor issues one.
                if b"\x1b[6n" in data:
                    os.write(self.fd, b"\x1b[1;1R")
        raise AssertionError(f"Missing {text!r}; last output: {plain[-2500:]}")

    def finish(self):
        end = time.monotonic() + 5
        while time.monotonic() < end:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.closed = True
                assert os.waitstatus_to_exitcode(status) == 0, status
                attrs = termios.tcgetattr(self.fd)
                assert attrs[3] & termios.ICANON, "terminal canonical mode was not restored"
                assert attrs[3] & termios.ECHO, "terminal echo was not restored"
                os.close(self.fd)
                return
            time.sleep(0.05)
        raise AssertionError("terminal did not exit")

    def close(self):
        if not self.closed:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except ProcessLookupError:
                pass
            os.close(self.fd)
            self.closed = True

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", nargs="?", default="target/debug/ai-sre")
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    with tempfile.TemporaryDirectory(prefix="ai-sre-terminal-") as directory:
        terminal = Terminal(binary, directory)
        try:
            terminal.expect("TERMINAL WORKSPACE")
            terminal.send("/serv\tcheckout-api\r")
            terminal.expect("Service: checkout-api")
            terminal.send("Why did checkout fail?\r")
            terminal.expect("REPORT READY")
            terminal.send("/review\r")
            terminal.expect("Checks for the next operator")
            terminal.send("/recall\r")
            terminal.expect("Past incident recall")
            terminal.send("/changes\r")
            terminal.expect("Two recorded runs are required")
            terminal.send("/handoff\r")
            terminal.expect("Incident handoff:")
            terminal.send(b"\x1b[A")
            terminal.expect("Why did checkout fail?")
            terminal.send(b"\x01\x0bCheck dependencies\r")
            terminal.expect("REPORT READY")
            terminal.send(b"\x12Why did checkout")
            terminal.expect("Why did checkout fail?")
            terminal.send(b"\x03")
            terminal.expect("Input cancelled")
            terminal.resize(40)
            terminal.send("/help\r")
            terminal.expect("Keyboard")
            terminal.send("/resume invalid\r")
            terminal.expect("Could not complete that request")
            terminal.send(b"\x04")
            terminal.expect("Session saved")
            terminal.finish()
        finally:
            terminal.close()
        db = Path(directory) / ".ai-sre/incidents.db"
        with sqlite3.connect(db) as connection:
            rows = connection.execute("SELECT text FROM shell_history ORDER BY id").fetchall()
            assert rows == [("Why did checkout fail?",), ("Check dependencies",)], rows
        # The editor restores redacted persisted questions on the next launch.
        terminal = Terminal(binary, directory, ["repl", "--service", "checkout-api"])
        try:
            terminal.expect("TERMINAL WORKSPACE")
            terminal.send(b"\x1b[A")
            terminal.expect("Check dependencies")
            terminal.send(b"\x03")
            terminal.expect("Input cancelled")
            terminal.send("/exit\r")
            terminal.expect("Session saved")
            terminal.finish()
        finally:
            terminal.close()
        started = threading.Event()
        release = threading.Event()
        class Model(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass
            def do_POST(self):
                self.rfile.read(int(self.headers.get("content-length", 0)))
                started.set()
                release.wait(10)
                try:
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(b'{"choices":[]}')
                except (BrokenPipeError, ConnectionResetError):
                    pass
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        config = Path(directory) / "cancel.toml"
        config.write_text(f'database = "cancel.db"\n[model]\nurl = "http://127.0.0.1:{server.server_port}/model"\nname = "slow-test-model"\n')
        terminal = Terminal(binary, directory, ["--config", str(config)])
        try:
            terminal.expect("TERMINAL WORKSPACE")
            terminal.send("Investigate cancellation\r")
            terminal.expect("Investigating")
            assert started.wait(5), "model request did not start"
            terminal.send(b"\x03", editing=False)
            terminal.expect("CANCELLED")
            terminal.send("/exit\r")
            terminal.expect("Session saved")
            terminal.finish()
            with sqlite3.connect(Path(directory) / "cancel.db") as connection:
                incident = json.loads(connection.execute("SELECT document FROM incidents").fetchone()[0])
                assert incident["status"] == "cancelled", incident["status"]
        finally:
            terminal.close()
            release.set()
            server.shutdown()
            server.server_close()
    print("PASS: default launch, Tab completion, arrow history, line editing, Ctrl+R search, Ctrl+C input/run cancellation, narrow resize, command-error recovery, persisted history, Ctrl+D and terminal restoration")

if __name__ == "__main__":
    main()
