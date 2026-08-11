from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
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
    if api_env is None or not tunnel_id:
        write_result(
            output,
            {
                "schema_version": 1,
                "status": "BLOCKED_EXTERNAL_CREDENTIAL",
                "api_key_present": api_env is not None,
                "tunnel_id_present": bool(tunnel_id),
                "secret_read_or_logged": False,
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
        try:
            deadline = time.monotonic() + args.timeout
            while time.monotonic() < deadline and process.poll() is None:
                if health_file.exists():
                    base = health_file.read_text(encoding="utf-8").strip().rstrip("/")
                    status = get_json(base + "/api/status")
                    if status and status.get("tunnel_metadata") and not status.get("tunnel_metadata_error"):
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

    ok = bool(status and status.get("tunnel_metadata") and not status.get("tunnel_metadata_error"))
    result = {
        "schema_version": 1,
        "status": "PASS" if ok else "FAIL",
        "authenticated_control_plane_metadata_observed": ok,
        "api_key_in_command_line": False,
        "api_key_reference": f"env:{api_env}",
        "tunnel_id_in_command_line": False,
        "credential_input": "stdin_to_child_environment" if args.credentials_stdin else "inherited_environment",
        "health_loopback": bool(status and str(status.get("health_listen_addr", "")).startswith("127.0.0.1:")),
        "metadata_error_present": bool(status and status.get("tunnel_metadata_error")),
        "stdout_nonempty": bool(stdout),
        "stderr_nonempty": bool(stderr),
        "secrets_emitted": False
    }
    write_result(output, result)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
