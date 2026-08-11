from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


def get_json(url: str) -> dict[str, Any] | None:
    try:
        with urllib.request.urlopen(url, timeout=5) as response:
            value = json.loads(response.read().decode("utf-8"))
    except Exception:
        return None
    return value if isinstance(value, dict) else None


def get_text(url: str) -> str | None:
    try:
        with urllib.request.urlopen(url, timeout=5) as response:
            return response.read().decode("utf-8", errors="replace")
    except Exception:
        return None


def http_status(url: str) -> int | None:
    try:
        with urllib.request.urlopen(url, timeout=5) as response:
            return int(response.status)
    except urllib.error.HTTPError as exc:
        return int(exc.code)
    except Exception:
        return None


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


_POLL_METRIC_RE = re.compile(
    r"^(commands_poll_cycles_total|commands_poll_errors_total|"
    r"commands_poll_last_successful_timestamp_seconds)(?:\{[^}]*\})?\s+"
    r"([-+0-9.eE]+)$"
)


def parse_poll_metrics(payload: str | None) -> dict[str, float]:
    values = {
        "cycles": 0.0,
        "errors": 0.0,
        "last_successful_timestamp_seconds": 0.0,
    }
    if not payload:
        return values
    for raw_line in payload.splitlines():
        match = _POLL_METRIC_RE.match(raw_line.strip())
        if not match:
            continue
        name, raw_value = match.groups()
        try:
            value = float(raw_value)
        except ValueError:
            continue
        if name == "commands_poll_cycles_total":
            values["cycles"] += value
        elif name == "commands_poll_errors_total":
            values["errors"] += value
        else:
            values["last_successful_timestamp_seconds"] = max(
                values["last_successful_timestamp_seconds"], value
            )
    return values


def write_result(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(value))


def main() -> int:
    parser = argparse.ArgumentParser(description="Credential-safe LB-000 live OpenAI Tunnel compatibility probe")
    parser.add_argument("--exe", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--timeout", type=float, default=45.0)
    parser.add_argument("--credentials-stdin", action="store_true")
    args = parser.parse_args()
    output = Path(args.output)

    stdin_api_key: str | None = None
    stdin_tunnel_id: str | None = None
    if args.credentials_stdin:
        try:
            payload = json.loads(sys.stdin.readline())
        except Exception:
            payload = {}
        if isinstance(payload, dict):
            api_value = payload.get("api_key")
            tunnel_value = payload.get("tunnel_id")
            stdin_api_key = api_value if isinstance(api_value, str) and api_value else None
            stdin_tunnel_id = tunnel_value if isinstance(tunnel_value, str) and tunnel_value else None

    api_env = (
        "CONTROL_PLANE_API_KEY"
        if stdin_api_key or os.environ.get("CONTROL_PLANE_API_KEY")
        else "OPENAI_API_KEY"
        if os.environ.get("OPENAI_API_KEY")
        else None
    )
    tunnel_id = stdin_tunnel_id or os.environ.get("CONTROL_PLANE_TUNNEL_ID")
    api_key = (stdin_api_key if stdin_api_key is not None else os.environ.get(api_env or ""))
    if api_env is None or not api_key or not tunnel_id:
        write_result(
            output,
            {
                "schema_version": 2,
                "status": "BLOCKED_EXTERNAL_CREDENTIAL",
                "api_key_present": bool(api_key),
                "tunnel_id_present": bool(tunnel_id),
                "measurement_status": "not_started_missing_credentials",
                "required_environment": ["CONTROL_PLANE_API_KEY or OPENAI_API_KEY", "CONTROL_PLANE_TUNNEL_ID"],
            },
        )
        return 2

    with tempfile.TemporaryDirectory(prefix="localbridge-lb000-live-tunnel-") as temp:
        health_file = Path(temp) / "health.url"
        argv = [
            str(Path(args.exe).resolve()),
            "run",
            "--embedded-mcp-stub",
            "--control-plane.api-key",
            f"env:{api_env}",
            "--control-plane.poll-timeout",
            "1000ms",
            "--control-plane.poll-deadline-guardrail",
            "500ms",
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
        if stdin_api_key is not None:
            child_env["CONTROL_PLANE_API_KEY"] = stdin_api_key
        if stdin_tunnel_id is not None:
            child_env["CONTROL_PLANE_TUNNEL_ID"] = stdin_tunnel_id
        process = subprocess.Popen(
            argv,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            env=child_env,
        )
        status: dict[str, Any] | None = None
        command_line: str | None = None
        ready_status: int | None = None
        poll_metrics = parse_poll_metrics(None)
        base: str | None = None
        working_state_observed = False
        try:
            deadline = time.monotonic() + args.timeout
            while time.monotonic() < deadline and process.poll() is None:
                if command_line is None:
                    command_line = process_command_line(process.pid)
                if health_file.exists():
                    base = health_file.read_text(encoding="utf-8").strip().rstrip("/")
                    status = get_json(base + "/api/status")
                    ready_status = http_status(base + "/readyz")
                    poll_metrics = parse_poll_metrics(get_text(base + "/metrics"))
                    metadata_ok = bool(
                        status
                        and status.get("tunnel_metadata")
                        and not status.get("tunnel_metadata_error")
                    )
                    poll_success = bool(
                        poll_metrics["cycles"] >= 1
                        and poll_metrics["last_successful_timestamp_seconds"] > 0
                    )
                    if metadata_ok and poll_success and ready_status == 200:
                        working_state_observed = process.poll() is None
                        break
                time.sleep(0.25)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            stdout, stderr = process.communicate()

    metadata_ok = bool(status and status.get("tunnel_metadata") and not status.get("tunnel_metadata_error"))
    poll_success = bool(
        poll_metrics["cycles"] >= 1
        and poll_metrics["last_successful_timestamp_seconds"] > 0
    )
    os_command_line_observed = command_line is not None
    api_key_in_command_line = None if command_line is None else api_key in command_line
    tunnel_id_in_command_line = None if command_line is None else tunnel_id in command_line
    env_reference_in_command_line = (
        None if command_line is None else f"env:{api_env}" in command_line
    )
    stdout_contains_api_key = api_key in stdout
    stderr_contains_api_key = api_key in stderr
    stdout_contains_tunnel_id = tunnel_id in stdout
    stderr_contains_tunnel_id = tunnel_id in stderr
    secrets_emitted = stdout_contains_api_key or stderr_contains_api_key
    health_loopback = bool(base and base.startswith("http://127.0.0.1:"))
    ok = bool(
        metadata_ok
        and poll_success
        and working_state_observed
        and ready_status == 200
        and health_loopback
        and os_command_line_observed
        and api_key_in_command_line is False
        and tunnel_id_in_command_line is False
        and env_reference_in_command_line is True
        and not secrets_emitted
    )
    result = {
        "schema_version": 2,
        "status": "PASS" if ok else "FAIL",
        "authenticated_control_plane_metadata_observed": metadata_ok,
        "control_plane_poll_success_observed": poll_success,
        "control_plane_poll_cycles_observed": int(poll_metrics["cycles"]),
        "control_plane_poll_errors_observed": int(poll_metrics["errors"]),
        "control_plane_poll_last_successful_timestamp_present": poll_success,
        "poll_evidence_source": "tunnel_client_prometheus_commands_poll_last_successful_timestamp_seconds",
        "tunnel_working_state_observed": working_state_observed,
        "ready_status": ready_status,
        "os_command_line_observed": os_command_line_observed,
        "api_key_in_command_line": api_key_in_command_line,
        "api_key_reference": f"env:{api_env}",
        "api_key_reference_in_command_line": env_reference_in_command_line,
        "tunnel_id_in_command_line": tunnel_id_in_command_line,
        "credential_input": "stdin_to_child_environment" if args.credentials_stdin else "inherited_environment",
        "health_loopback": health_loopback,
        "metadata_error_present": bool(status and status.get("tunnel_metadata_error")),
        "stdout_nonempty": bool(stdout),
        "stderr_nonempty": bool(stderr),
        "stdout_contains_api_key": stdout_contains_api_key,
        "stderr_contains_api_key": stderr_contains_api_key,
        "stdout_contains_tunnel_id": stdout_contains_tunnel_id,
        "stderr_contains_tunnel_id": stderr_contains_tunnel_id,
        "secrets_emitted": secrets_emitted,
    }
    write_result(output, result)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
