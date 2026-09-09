from __future__ import annotations

import ctypes
import os
import signal
import subprocess
import threading
import time
from dataclasses import dataclass, field
from typing import Any, BinaryIO

from .errors import ToolFailure
from .textutils import DEFAULT_MAX_LINES, TextTruncation, truncate_text_tail


SESSION_BUFFER_BYTES = 524_288
HARD_KILL_SIGNAL = getattr(signal, "SIGKILL", signal.SIGTERM)


def _windows_force_kill_tree(process: subprocess.Popen[bytes]) -> None:
    system_root = os.environ.get("SystemRoot", r"C:\Windows")
    taskkill = os.path.join(system_root, "System32", "taskkill.exe")
    try:
        subprocess.run(
            [taskkill, "/PID", str(process.pid), "/T", "/F"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=0.25,
            check=False,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
    except (OSError, subprocess.TimeoutExpired):
        try:
            process.kill()
        except OSError:
            pass


def terminate_process_group(
    process: subprocess.Popen[bytes],
    signum: signal.Signals,
    *,
    force: bool = False,
) -> None:
    """Request termination without owning the lifecycle wait budget.

    Callers that expose a wait contract must observe the process against their
    own single deadline. Keeping waits here used to reset that budget once for
    TERM, once for tree kill, and once again for pipe cleanup.
    """
    if not hasattr(os, "killpg"):
        if os.name == "nt":
            if not force and signum == signal.SIGINT:
                event = getattr(signal, "CTRL_BREAK_EVENT", None)
                if event is not None:
                    try:
                        process.send_signal(event)
                        return
                    except OSError:
                        pass
            # shell=True inserts cmd.exe between the session and the requested
            # program. Terminating only that root loses ownership of a still
            # running child and leaves its inherited pipes open. Windows has no
            # group TERM primitive, so terminate the owned tree in one action.
            _windows_force_kill_tree(process)
            return
        try:
            if force:
                process.kill()
            else:
                process.terminate()
        except OSError:
            if not force:
                try:
                    process.kill()
                except OSError:
                    pass
        return
    try:
        os.killpg(process.pid, signum)
    except ProcessLookupError:
        return
    except Exception:
        process.terminate()

def spawn_process(
    command: Any,
    *,
    cwd: str,
    shell: bool,
    env: dict[str, str],
    tty: bool,
    popen_kwargs: dict[str, Any],
) -> tuple[subprocess.Popen[bytes], int | None]:
    """Spawn a pipe-backed or true POSIX PTY-backed process."""

    if not tty:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            shell=shell,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            **popen_kwargs,
        )
        return process, None
    if os.name == "nt":
        raise ToolFailure(
            "TTY_UNSUPPORTED",
            "tty=true requires ConPTY support, which is not available in this build.",
            category="runtime",
            details={"platform": os.name, "retry_hint": "Run the command without tty=true."},
        )
    try:
        import pty

        master_fd, slave_fd = pty.openpty()
    except (ImportError, OSError) as exc:
        raise ToolFailure(
            "TTY_UNSUPPORTED",
            "A POSIX pseudo-terminal could not be created.",
            category="runtime",
        ) from exc
    try:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            shell=shell,
            stdin=slave_fd,
            stdout=slave_fd,
            stderr=slave_fd,
            env=env,
            **popen_kwargs,
        )
    except Exception:
        os.close(master_fd)
        raise
    finally:
        os.close(slave_fd)
    return process, master_fd


@dataclass
class ExecSession:
    session_id: str
    process: subprocess.Popen[bytes]
    timeout_at: float | None = None
    warnings: list[str] = field(default_factory=list)
    stdout: bytearray = field(default_factory=bytearray)
    stderr: bytearray = field(default_factory=bytearray)
    stdout_start_offset: int = 0
    stderr_start_offset: int = 0
    stdout_cursor: int = 0
    stderr_cursor: int = 0
    stdout_total_bytes: int = 0
    stderr_total_bytes: int = 0
    stdout_dropped_bytes: int = 0
    stderr_dropped_bytes: int = 0
    buffer_limit: int = SESSION_BUFFER_BYTES
    lock: threading.Lock = field(default_factory=threading.Lock)
    reader_threads: list[threading.Thread] = field(default_factory=list)
    watchdog_thread: threading.Thread | None = None
    watchdog_stop: threading.Event = field(default_factory=threading.Event)
    started_at: float = field(default_factory=time.time)
    completed_at: float | None = None
    closed: bool = False
    exit_code: int | None = None
    signal_name: str | None = None
    timed_out: bool = False
    terminating: bool = False
    pty_master_fd: int | None = None
    output_encoding: str = "utf-8"
    _stdin_closed: bool = False

    @property
    def retained_bytes(self) -> int:
        with self.lock:
            return len(self.stdout) + len(self.stderr)

    def append_stdout(self, chunk: bytes) -> None:
        with self.lock:
            self.stdout.extend(chunk)
            self.stdout_total_bytes += len(chunk)
            self.stdout_dropped_bytes += _trim_buffer(
                self.stdout,
                total_bytes=self.stdout_total_bytes,
                start_offset_attr="stdout_start_offset",
                session=self,
            )

    def append_stderr(self, chunk: bytes) -> None:
        with self.lock:
            self.stderr.extend(chunk)
            self.stderr_total_bytes += len(chunk)
            self.stderr_dropped_bytes += _trim_buffer(
                self.stderr,
                total_bytes=self.stderr_total_bytes,
                start_offset_attr="stderr_start_offset",
                session=self,
            )

    def write_input(self, data: bytes) -> None:
        if self._stdin_closed:
            raise ToolFailure("SESSION_CLOSED", "Session stdin is closed.", category="runtime")
        try:
            if self.pty_master_fd is not None:
                os.write(self.pty_master_fd, data)
                return
            if self.process.stdin is None or self.process.stdin.closed:
                raise ToolFailure("SESSION_CLOSED", "Session stdin is closed.", category="runtime")
            self.process.stdin.write(data)
            self.process.stdin.flush()
        except (BrokenPipeError, OSError, ValueError) as exc:
            raise ToolFailure("SESSION_CLOSED", "Session stdin is closed.", category="runtime") from exc

    def close_stdin(self) -> None:
        if self.pty_master_fd is not None or self._stdin_closed:
            return
        self._stdin_closed = True
        if self.process.stdin is not None:
            try:
                self.process.stdin.close()
            except OSError:
                pass

    def snapshot_since_cursor(self, max_output_bytes: int) -> dict[str, Any]:
        self.refresh_status()
        with self.lock:
            stdout_omitted = max(0, self.stdout_start_offset - self.stdout_cursor)
            stderr_omitted = max(0, self.stderr_start_offset - self.stderr_cursor)
            stdout_start = max(0, self.stdout_cursor - self.stdout_start_offset)
            stderr_start = max(0, self.stderr_cursor - self.stderr_start_offset)
            stdout_bytes = bytes(self.stdout[stdout_start:])
            stderr_bytes = bytes(self.stderr[stderr_start:])
            self.stdout_cursor = self.stdout_total_bytes
            self.stderr_cursor = self.stderr_total_bytes
        stdout_truncation = truncate_output_bytes_tail(
            stdout_bytes, max_output_bytes, encoding=self.output_encoding
        )
        stderr_truncation = truncate_output_bytes_tail(
            stderr_bytes, max_output_bytes, encoding=self.output_encoding
        )
        if self.timed_out:
            status = "timeout"
        elif self.terminating and self.process.poll() is None:
            status = "running"
        elif self.signal_name is not None:
            status = "terminated"
        else:
            status = "running" if self.process.poll() is None else "exited"
        payload: dict[str, Any] = {
            "session_id": self.session_id,
            "status": status,
            "exit_code": self.exit_code,
            "signal": self.signal_name,
            "timed_out": self.timed_out,
            "stdout": stdout_truncation.content,
            "stderr": stderr_truncation.content,
            "stdout_truncated": stdout_truncation.truncated,
            "stderr_truncated": stderr_truncation.truncated,
            "stdout_truncated_by": stdout_truncation.truncated_by,
            "stderr_truncated_by": stderr_truncation.truncated_by,
            "stdout_output_lines": stdout_truncation.output_lines,
            "stderr_output_lines": stderr_truncation.output_lines,
            "stdout_output_bytes": stdout_truncation.output_bytes,
            "stderr_output_bytes": stderr_truncation.output_bytes,
            "stdout_dropped_bytes": self.stdout_dropped_bytes,
            "stderr_dropped_bytes": self.stderr_dropped_bytes,
            "stdout_omitted_bytes": stdout_omitted,
            "stderr_omitted_bytes": stderr_omitted,
            "truncated": (
                stdout_truncation.truncated
                or stderr_truncation.truncated
                or stdout_omitted > 0
                or stderr_omitted > 0
            ),
            "ok": True,
        }
        warnings: list[str] = list(self.warnings)
        if stdout_truncation.truncated:
            warnings.append(f"stdout truncated from tail by {stdout_truncation.truncated_by}")
        if stderr_truncation.truncated:
            warnings.append(f"stderr truncated from tail by {stderr_truncation.truncated_by}")
        if stdout_omitted > 0:
            warnings.append("stdout cursor skipped dropped bytes")
        if stderr_omitted > 0:
            warnings.append("stderr cursor skipped dropped bytes")
        if warnings:
            payload["warnings"] = warnings
        return payload

    def refresh_status(self) -> None:
        if self.timeout_at is not None and not self.timed_out and self.process.poll() is None and time.time() >= self.timeout_at:
            self.timed_out = True
            terminate_process_group(self.process, signal.SIGTERM)
            self.drain_readers()
        code = self.process.poll()
        if code is None:
            return
        self.watchdog_stop.set()
        self.drain_readers()
        self.exit_code = code
        self.terminating = False
        if code < 0:
            values = {item.value for item in signal.Signals}
            self.signal_name = signal.Signals(-code).name if -code in values else str(-code)
        self.closed = True
        if self.completed_at is None:
            self.completed_at = time.time()

    def drain_readers(self, timeout: float = 0.2) -> None:
        deadline = time.time() + timeout
        for thread in list(self.reader_threads):
            remaining = max(0.0, deadline - time.time())
            if remaining <= 0:
                break
            thread.join(timeout=remaining)

    def drain_watchdog(self, timeout: float = 0.2) -> None:
        thread = self.watchdog_thread
        if thread is None or thread is threading.current_thread():
            return
        thread.join(timeout=max(0.0, timeout))
        if not thread.is_alive():
            self.watchdog_thread = None

    def retained_output_bytes(self) -> bytes:
        with self.lock:
            stdout = bytes(self.stdout)
            stderr = bytes(self.stderr)
        sections: list[bytes] = []
        if stdout:
            sections.extend([b"--- stdout ---\n", stdout])
        if stderr:
            if sections:
                sections.append(b"\n")
            sections.extend([b"--- stderr ---\n", stderr])
        return b"".join(sections)

    def retained_stream_bytes(self, stream: str) -> tuple[bytes, int, int, int]:
        with self.lock:
            if stream == "stdout":
                return bytes(self.stdout), self.stdout_start_offset, self.stdout_total_bytes, self.stdout_dropped_bytes
            if stream == "stderr":
                return bytes(self.stderr), self.stderr_start_offset, self.stderr_total_bytes, self.stderr_dropped_bytes
        raise ValueError(f"Unknown output stream: {stream}")


@dataclass
class RetainedExecOutput:
    """Terminal output snapshot without process, pipe, or thread ownership."""

    session_id: str
    warnings: list[str]
    stdout: bytearray
    stderr: bytearray
    stdout_start_offset: int
    stderr_start_offset: int
    stdout_cursor: int
    stderr_cursor: int
    stdout_total_bytes: int
    stderr_total_bytes: int
    stdout_dropped_bytes: int
    stderr_dropped_bytes: int
    completed_at: float
    exit_code: int | None
    signal_name: str | None
    timed_out: bool
    output_encoding: str
    lock: Any = field(repr=False)

    @classmethod
    def capture(cls, session: ExecSession, registry_lock: Any) -> RetainedExecOutput:
        session.refresh_status()
        if session.process.poll() is None:
            raise ValueError("cannot retain output for a running process")
        session.close_stdin()
        session.drain_readers(timeout=0.2)
        session.drain_watchdog(timeout=0.2)
        for stream in (session.process.stdout, session.process.stderr):
            if stream is not None and not stream.closed:
                try:
                    stream.close()
                except OSError:
                    pass
        session.reader_threads.clear()
        with session.lock:
            retained = cls(
                session_id=session.session_id,
                warnings=list(session.warnings),
                stdout=bytearray(session.stdout),
                stderr=bytearray(session.stderr),
                stdout_start_offset=session.stdout_start_offset,
                stderr_start_offset=session.stderr_start_offset,
                stdout_cursor=session.stdout_cursor,
                stderr_cursor=session.stderr_cursor,
                stdout_total_bytes=session.stdout_total_bytes,
                stderr_total_bytes=session.stderr_total_bytes,
                stdout_dropped_bytes=session.stdout_dropped_bytes,
                stderr_dropped_bytes=session.stderr_dropped_bytes,
                completed_at=session.completed_at or time.time(),
                exit_code=session.exit_code,
                signal_name=session.signal_name,
                timed_out=session.timed_out,
                output_encoding=session.output_encoding,
                # Retained output has no independent lifecycle owner. Reuse
                # the Runtime registry's re-entrant lock so a retained record
                # does not allocate one Windows synchronization handle per
                # completed command.
                lock=registry_lock,
            )
        release_terminal_process_resources(session)
        return retained

    @property
    def retained_bytes(self) -> int:
        with self.lock:
            return len(self.stdout) + len(self.stderr)

    def refresh_status(self) -> None:
        return

    def snapshot_since_cursor(self, max_output_bytes: int) -> dict[str, Any]:
        with self.lock:
            stdout_omitted = max(0, self.stdout_start_offset - self.stdout_cursor)
            stderr_omitted = max(0, self.stderr_start_offset - self.stderr_cursor)
            stdout_start = max(0, self.stdout_cursor - self.stdout_start_offset)
            stderr_start = max(0, self.stderr_cursor - self.stderr_start_offset)
            stdout_bytes = bytes(self.stdout[stdout_start:])
            stderr_bytes = bytes(self.stderr[stderr_start:])
            self.stdout_cursor = self.stdout_total_bytes
            self.stderr_cursor = self.stderr_total_bytes
        stdout_truncation = truncate_output_bytes_tail(
            stdout_bytes, max_output_bytes, encoding=self.output_encoding
        )
        stderr_truncation = truncate_output_bytes_tail(
            stderr_bytes, max_output_bytes, encoding=self.output_encoding
        )
        status = "timeout" if self.timed_out else "terminated" if self.signal_name is not None else "exited"
        payload: dict[str, Any] = {
            "session_id": self.session_id,
            "status": status,
            "exit_code": self.exit_code,
            "signal": self.signal_name,
            "timed_out": self.timed_out,
            "stdout": stdout_truncation.content,
            "stderr": stderr_truncation.content,
            "stdout_truncated": stdout_truncation.truncated,
            "stderr_truncated": stderr_truncation.truncated,
            "stdout_truncated_by": stdout_truncation.truncated_by,
            "stderr_truncated_by": stderr_truncation.truncated_by,
            "stdout_output_lines": stdout_truncation.output_lines,
            "stderr_output_lines": stderr_truncation.output_lines,
            "stdout_output_bytes": stdout_truncation.output_bytes,
            "stderr_output_bytes": stderr_truncation.output_bytes,
            "stdout_dropped_bytes": self.stdout_dropped_bytes,
            "stderr_dropped_bytes": self.stderr_dropped_bytes,
            "stdout_omitted_bytes": stdout_omitted,
            "stderr_omitted_bytes": stderr_omitted,
            "truncated": (
                stdout_truncation.truncated
                or stderr_truncation.truncated
                or stdout_omitted > 0
                or stderr_omitted > 0
            ),
            "ok": True,
        }
        warnings = list(self.warnings)
        if stdout_truncation.truncated:
            warnings.append(f"stdout truncated from tail by {stdout_truncation.truncated_by}")
        if stderr_truncation.truncated:
            warnings.append(f"stderr truncated from tail by {stderr_truncation.truncated_by}")
        if stdout_omitted > 0:
            warnings.append("stdout cursor skipped dropped bytes")
        if stderr_omitted > 0:
            warnings.append("stderr cursor skipped dropped bytes")
        if warnings:
            payload["warnings"] = warnings
        return payload

    def retained_output_bytes(self) -> bytes:
        with self.lock:
            stdout = bytes(self.stdout)
            stderr = bytes(self.stderr)
        sections: list[bytes] = []
        if stdout:
            sections.extend([b"--- stdout ---\n", stdout])
        if stderr:
            if sections:
                sections.append(b"\n")
            sections.extend([b"--- stderr ---\n", stderr])
        return b"".join(sections)

    def retained_stream_bytes(self, stream: str) -> tuple[bytes, int, int, int]:
        with self.lock:
            if stream == "stdout":
                return bytes(self.stdout), self.stdout_start_offset, self.stdout_total_bytes, self.stdout_dropped_bytes
            if stream == "stderr":
                return bytes(self.stderr), self.stderr_start_offset, self.stderr_total_bytes, self.stderr_dropped_bytes
        raise ValueError(f"Unknown output stream: {stream}")


def release_terminal_process_resources(session: ExecSession) -> None:
    """Release every OS owner after terminal metadata and output are copied."""

    process = session.process
    if process.returncode is None:
        raise ValueError("cannot release resources for a running process")
    for name in ("stdin", "stdout", "stderr"):
        stream = getattr(process, name, None)
        if stream is not None and not stream.closed:
            try:
                stream.close()
            except OSError:
                pass
        setattr(process, name, None)
    if session.pty_master_fd is not None:
        try:
            os.close(session.pty_master_fd)
        except OSError:
            pass
        session.pty_master_fd = None
    if os.name == "nt":
        handle = getattr(process, "_handle", None)
        if handle is not None:
            try:
                handle.Close()
            except OSError:
                pass
            process._handle = None


def start_reader_threads(session: ExecSession) -> None:
    def reader(stream: BinaryIO, append: Any) -> None:
        try:
            while True:
                chunk = os.read(stream.fileno(), 4096)
                if not chunk:
                    break
                append(chunk)
        except (OSError, ValueError):
            return
        finally:
            try:
                stream.close()
            except OSError:
                pass

    def pty_reader(fd: int) -> None:
        try:
            while True:
                chunk = os.read(fd, 4096)
                if not chunk:
                    break
                session.append_stdout(chunk)
        except OSError:
            return
        finally:
            try:
                os.close(fd)
            except OSError:
                pass
            if session.pty_master_fd == fd:
                session.pty_master_fd = None

    if session.pty_master_fd is not None:
        thread = threading.Thread(target=pty_reader, args=(session.pty_master_fd,), daemon=True)
        session.reader_threads.append(thread)
        thread.start()
        return
    if session.process.stdout is not None:
        thread = threading.Thread(target=reader, args=(session.process.stdout, session.append_stdout), daemon=True)
        session.reader_threads.append(thread)
        thread.start()
    if session.process.stderr is not None:
        thread = threading.Thread(target=reader, args=(session.process.stderr, session.append_stderr), daemon=True)
        session.reader_threads.append(thread)
        thread.start()


def start_session_watchdog(session: ExecSession) -> None:
    if session.timeout_at is None:
        return

    def watchdog() -> None:
        delay = max(0.0, session.timeout_at - time.time()) if session.timeout_at is not None else 0.0
        deadline = time.time() + delay
        while session.process.poll() is None and time.time() < deadline:
            if session.watchdog_stop.wait(timeout=min(0.05, max(0.0, deadline - time.time()))):
                return
        if session.process.poll() is not None or session.timed_out:
            session.refresh_status()
            return
        session.timed_out = True
        terminate_process_group(session.process, signal.SIGTERM)
        session.refresh_status()

    thread = threading.Thread(
        target=watchdog,
        name=f"coding-tools-watchdog-{session.session_id}",
        daemon=True,
    )
    session.watchdog_thread = thread
    thread.start()


def _trim_buffer(
    buffer: bytearray,
    *,
    total_bytes: int,
    start_offset_attr: str,
    session: ExecSession,
) -> int:
    overflow = len(buffer) - session.buffer_limit
    if overflow <= 0:
        return 0
    del buffer[:overflow]
    setattr(session, start_offset_attr, total_bytes - len(buffer))
    return overflow


def decode_output_bytes(data: bytes, encoding: str = "utf-8") -> str:
    if encoding == "utf-8":
        return data.decode("utf-8", errors="replace")
    if os.name != "nt" or encoding not in {"windows_oem", "windows_acp"}:
        return data.decode("utf-8", errors="replace")
    try:
        # Windows console/native programs can emit UTF-8 when the inherited host
        # code page is UTF-8. Accept it only when the byte stream is strictly valid;
        # otherwise use the explicitly declared OEM/ACP fallback below.
        return data.decode("utf-8", errors="strict")
    except UnicodeDecodeError:
        pass
    code_page = (
        ctypes.windll.kernel32.GetOEMCP()
        if encoding == "windows_oem"
        else ctypes.windll.kernel32.GetACP()
    )
    return data.decode(f"cp{code_page}", errors="replace")


def truncate_output_bytes_tail(
    data: bytes,
    max_bytes: int,
    max_lines: int = DEFAULT_MAX_LINES,
    *,
    encoding: str = "utf-8",
) -> TextTruncation:
    return truncate_text_tail(
        decode_output_bytes(data, encoding),
        max_lines=max_lines,
        max_bytes=max_bytes,
    )
