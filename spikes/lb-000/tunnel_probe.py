from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path
from typing import Any


def http_status(url: str) -> int | None:
    try:
        with urllib.request.urlopen(url, timeout=3) as response:
            return int(response.status)
    except urllib.error.HTTPError as exc:
        return int(exc.code)
    except Exception:
        return None


def http_json(url: str) -> dict[str, Any] | None:
    try:
        with urllib.request.urlopen(url, timeout=3) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except Exception:
        return None
    return payload if isinstance(payload, dict) else None


def process_command_line(pid: int) -> str | None:
    command = (
        "$p=Get-CimInstance Win32_Process -Filter \"ProcessId=%d\"; "
        "if($p){[Console]::Out.Write($p.CommandLine)}" % pid
    )
    try:
        completed = subprocess.run(
            ["powershell.exe", "-NoProfile", "-Command", command],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=5,
        )
    except Exception:
        return None
    value = completed.stdout.strip()
    return value or None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    exe = str(Path(args.exe).resolve())
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    sentinel = "LB000_SECRET_" + uuid.uuid4().hex

    with tempfile.TemporaryDirectory(prefix="localbridge-lb000-tunnel-") as temp:
        temp_path = Path(temp)
        health_file = temp_path / "health.url"
        argv = [
            exe,
            "run",
            "--embedded-mcp-stub",
            "--control-plane.tunnel-id",
            "tunnel_00000000000000000000000000000000",
            "--control-plane.api-key",
            "env:LOCALBRIDGE_RUNTIME_API_KEY",
            "--control-plane.base-url",
            "http://127.0.0.1:9",
            "--health.listen-addr",
            "127.0.0.1:0",
            "--health.url-file",
            str(health_file),
            "--log.format",
            "struct-text",
            "--log.level",
            "warn",
        ]
        child_env = os.environ.copy()
        child_env["LOCALBRIDGE_RUNTIME_API_KEY"] = sentinel
        process = subprocess.Popen(
            argv,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            env=child_env,
        )
        command_line: str | None = None
        health_url: str | None = None
        try:
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                if command_line is None and process.poll() is None:
                    command_line = process_command_line(process.pid)
                if health_file.exists():
                    health_url = health_file.read_text(encoding="utf-8").strip()
                    break
                if process.poll() is not None:
                    break
                time.sleep(0.1)
            health = http_status(health_url.rstrip("/") + "/healthz") if health_url else None
            ready = http_status(health_url.rstrip("/") + "/readyz") if health_url else None
            status_payload = http_json(health_url.rstrip("/") + "/api/status") if health_url else None
            system_payload = http_json(health_url.rstrip("/") + "/api/system") if health_url else None
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            stdout, stderr = process.communicate()

    args_text = " ".join(argv)
    result: dict[str, Any] = {
        "schema_version": 1,
        "binary_started": True,
        "process_exit_code_after_probe_stop": process.returncode,
        "argv_contains_secret": sentinel in args_text,
        "argv_uses_env_reference": "env:LOCALBRIDGE_RUNTIME_API_KEY" in argv,
        "os_command_line_observed": command_line is not None,
        "os_command_line_contains_secret": None if command_line is None else sentinel in command_line,
        "os_command_line_uses_env_reference": None
        if command_line is None
        else "env:LOCALBRIDGE_RUNTIME_API_KEY" in command_line,
        "secret_reference": "env:LOCALBRIDGE_RUNTIME_API_KEY",
        "child_environment_only": True,
        "persistent_environment_modified": False,
        "health_url": health_url,
        "health_loopback": bool(health_url and health_url.startswith("http://127.0.0.1:")),
        "health_status": health,
        "ready_status": ready,
        "readyz_is_process_startup_readiness_only": True,
        "readyz_does_not_prove_control_plane_connectivity": ready == 200,
        "admin_status": status_payload,
        "admin_system": system_payload,
        "control_plane_proxy_health": None if system_payload is None else system_payload.get("proxy_health"),
        "stdout_contains_secret": sentinel in stdout,
        "stderr_contains_secret": sentinel in stderr,
        "live_external_credentials_present": bool(os.environ.get("CONTROL_PLANE_API_KEY") or os.environ.get("OPENAI_API_KEY")),
        "live_external_test_status": "AVAILABLE" if (os.environ.get("CONTROL_PLANE_API_KEY") or os.environ.get("OPENAI_API_KEY")) else "BLOCKED_EXTERNAL_CREDENTIAL",
    }
    result["ok"] = bool(
        not result["argv_contains_secret"]
        and result["argv_uses_env_reference"]
        and result["os_command_line_contains_secret"] is not True
        and result["os_command_line_uses_env_reference"] is not False
        and result["health_loopback"]
        and not result["stdout_contains_secret"]
        and not result["stderr_contains_secret"]
    )
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
