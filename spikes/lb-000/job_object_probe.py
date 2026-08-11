from __future__ import annotations

import ctypes
import json
import os
import subprocess
import sys
import tempfile
import time
from ctypes import wintypes
from pathlib import Path

JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000
JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS = 9
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
STILL_ACTIVE = 259


class IO_COUNTERS(ctypes.Structure):
    _fields_ = [
        ("ReadOperationCount", ctypes.c_ulonglong),
        ("WriteOperationCount", ctypes.c_ulonglong),
        ("OtherOperationCount", ctypes.c_ulonglong),
        ("ReadTransferCount", ctypes.c_ulonglong),
        ("WriteTransferCount", ctypes.c_ulonglong),
        ("OtherTransferCount", ctypes.c_ulonglong),
    ]


class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("PerProcessUserTimeLimit", ctypes.c_longlong),
        ("PerJobUserTimeLimit", ctypes.c_longlong),
        ("LimitFlags", wintypes.DWORD),
        ("MinimumWorkingSetSize", ctypes.c_size_t),
        ("MaximumWorkingSetSize", ctypes.c_size_t),
        ("ActiveProcessLimit", wintypes.DWORD),
        ("Affinity", ctypes.c_size_t),
        ("PriorityClass", wintypes.DWORD),
        ("SchedulingClass", wintypes.DWORD),
    ]


class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION),
        ("IoInfo", IO_COUNTERS),
        ("ProcessMemoryLimit", ctypes.c_size_t),
        ("JobMemoryLimit", ctypes.c_size_t),
        ("PeakProcessMemoryUsed", ctypes.c_size_t),
        ("PeakJobMemoryUsed", ctypes.c_size_t),
    ]


kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
kernel32.CreateJobObjectW.restype = wintypes.HANDLE
kernel32.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
kernel32.SetInformationJobObject.restype = wintypes.BOOL
kernel32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
kernel32.CloseHandle.restype = wintypes.BOOL
kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
kernel32.OpenProcess.restype = wintypes.HANDLE
kernel32.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
kernel32.GetExitCodeProcess.restype = wintypes.BOOL


def win_error(label: str) -> OSError:
    code = ctypes.get_last_error()
    return OSError(code, f"{label}: {ctypes.FormatError(code)}")


def pid_alive(pid: int) -> bool:
    handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not handle:
        return False
    try:
        code = wintypes.DWORD()
        if not kernel32.GetExitCodeProcess(handle, ctypes.byref(code)):
            return False
        return code.value == STILL_ACTIVE
    finally:
        kernel32.CloseHandle(handle)


def main() -> int:
    if os.name != "nt":
        print(json.dumps({"ok": False, "reason": "Windows required"}))
        return 1
    output = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("spikes/lb-000/job-object-result.json")
    output.parent.mkdir(parents=True, exist_ok=True)

    job = kernel32.CreateJobObjectW(None, None)
    if not job:
        raise win_error("CreateJobObjectW")
    info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    if not kernel32.SetInformationJobObject(
        job,
        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
        ctypes.byref(info),
        ctypes.sizeof(info),
    ):
        kernel32.CloseHandle(job)
        raise win_error("SetInformationJobObject")

    with tempfile.TemporaryDirectory(prefix="localbridge-lb000-job-") as temp:
        temp_path = Path(temp)
        start_file = temp_path / "start"
        child_pid_file = temp_path / "child.pid"
        helper = (
            "import pathlib,subprocess,sys,time; "
            f"start=pathlib.Path({str(start_file)!r}); pidfile=pathlib.Path({str(child_pid_file)!r}); "
            "\nwhile not start.exists(): time.sleep(0.01)\n"
            "p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); "
            "pidfile.write_text(str(p.pid), encoding='ascii'); time.sleep(60)"
        )
        root = subprocess.Popen([sys.executable, "-c", helper])
        assigned = False
        try:
            assigned = bool(kernel32.AssignProcessToJobObject(job, wintypes.HANDLE(int(root._handle))))  # type: ignore[attr-defined]
            if not assigned:
                raise win_error("AssignProcessToJobObject")
            start_file.write_text("go", encoding="ascii")
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and not child_pid_file.exists():
                time.sleep(0.02)
            if not child_pid_file.exists():
                raise RuntimeError("nested child pid was not published")
            child_pid = int(child_pid_file.read_text(encoding="ascii"))
            before = {"root_alive": root.poll() is None, "nested_alive": pid_alive(child_pid)}
            kernel32.CloseHandle(job)
            job = None
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and (root.poll() is None or pid_alive(child_pid)):
                time.sleep(0.05)
            after = {"root_alive": root.poll() is None, "nested_alive": pid_alive(child_pid)}
            result = {
                "schema_version": 1,
                "ok": assigned and before["root_alive"] and before["nested_alive"] and not after["root_alive"] and not after["nested_alive"],
                "job_limit": "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE",
                "assigned_before_nested_spawn": True,
                "root_pid": root.pid,
                "nested_pid": child_pid,
                "before_job_close": before,
                "after_job_close": after,
                "ownership_model": "kernel job handle, not PID-only",
            }
            output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
            print(json.dumps(result))
            return 0 if result["ok"] else 1
        finally:
            if job:
                kernel32.CloseHandle(job)
            if root.poll() is None:
                root.kill()
                root.wait(timeout=5)


if __name__ == "__main__":
    raise SystemExit(main())
