from __future__ import annotations

import argparse
import hashlib
import json
import os
import secrets
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

PROTOCOL_VERSION = "2025-11-25"


class McpClient:
    def __init__(self, endpoint: str, auth_token: str | None = None) -> None:
        self.endpoint = endpoint
        self.auth_token = auth_token
        self.session_id: str | None = None
        self.next_id = 1

    def _post(self, method: str, params: dict[str, Any], *, notification: bool = False) -> dict[str, Any] | None:
        payload: dict[str, Any] = {"jsonrpc": "2.0", "method": method, "params": params}
        if not notification:
            payload["id"] = self.next_id
            self.next_id += 1
        headers = {
            "Accept": "application/json, text/event-stream",
            "Content-Type": "application/json",
            "MCP-Protocol-Version": PROTOCOL_VERSION,
        }
        if self.session_id:
            headers["Mcp-Session-Id"] = self.session_id
        if self.auth_token:
            headers["Authorization"] = f"Bearer {self.auth_token}"
        request = urllib.request.Request(
            self.endpoint,
            data=json.dumps(payload, separators=(",", ":")).encode("utf-8"),
            headers=headers,
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                raw = response.read().decode("utf-8", errors="replace").strip()
                for key, value in response.headers.items():
                    if key.lower() == "mcp-session-id":
                        self.session_id = value
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode("utf-8", errors="replace").strip()
            raise RuntimeError(f"MCP HTTP {exc.code}: {raw}") from exc
        if notification:
            return None
        if raw.startswith("event:") or raw.startswith("data:"):
            data_line = next((line[5:].strip() for line in raw.splitlines() if line.startswith("data:")), "")
            raw = data_line
        reply = json.loads(raw)
        if "error" in reply:
            raise RuntimeError(f"MCP JSON-RPC error: {reply['error']}")
        return reply["result"]

    def initialize(self) -> dict[str, Any]:
        result = self._post(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "localbridge-lb000-probe", "version": "1"},
            },
        )
        assert isinstance(result, dict)
        self._post("notifications/initialized", {}, notification=True)
        return result

    def list_tools(self) -> list[dict[str, Any]]:
        result = self._post("tools/list", {})
        assert isinstance(result, dict) and isinstance(result.get("tools"), list)
        return result["tools"]

    def call(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        result = self._post("tools/call", {"name": name, "arguments": arguments})
        assert isinstance(result, dict)
        return result


def canonical_hash(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def structured(result: dict[str, Any]) -> dict[str, Any]:
    value = result.get("structuredContent")
    if isinstance(value, dict):
        return value
    return {}


def is_error(result: dict[str, Any]) -> bool:
    return result.get("isError") is True or structured(result).get("ok") is False


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_client(endpoint: str, auth_token: str, timeout: float = 10.0) -> tuple[McpClient, dict[str, Any]]:
    deadline = time.monotonic() + timeout
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        client = McpClient(endpoint, auth_token)
        try:
            return client, client.initialize()
        except Exception as exc:  # noqa: BLE001 - probe records startup failures
            last_error = exc
            time.sleep(0.1)
    raise RuntimeError(f"MCP did not become ready: {last_error}")


def run_server(workspace: Path, mode: str) -> tuple[subprocess.Popen[str], McpClient, dict[str, Any], list[dict[str, Any]]]:
    port = free_port()
    endpoint = f"http://127.0.0.1:{port}/mcp"
    auth_token = secrets.token_urlsafe(32)
    env = os.environ.copy()
    env["CODING_TOOLS_MCP_TELEMETRY"] = "off"
    env["DO_NOT_TRACK"] = "1"
    env["CODING_TOOLS_MCP_AUTH_TOKEN"] = auth_token
    process = subprocess.Popen(
        [
            sys.executable,
            "-m",
            "coding_tools_mcp",
            "--workspace",
            str(workspace),
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
            "--permission-mode",
            mode,
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=env,
    )
    try:
        client, initialize = wait_client(endpoint, auth_token)
        tools = client.list_tools()
        return process, client, initialize, tools
    except Exception:
        process.terminate()
        stdout, stderr = process.communicate(timeout=5)
        raise RuntimeError(f"server startup failed; stdout={stdout!r}; stderr={stderr!r}")


def stop_server(process: subprocess.Popen[str]) -> dict[str, Any]:
    process.terminate()
    try:
        stdout, stderr = process.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        stdout, stderr = process.communicate(timeout=5)
    return {"exit_code": process.returncode, "stdout": stdout, "stderr": stderr}


def create_junction(link: Path, target: Path) -> dict[str, Any]:
    completed = subprocess.run(
        ["cmd.exe", "/d", "/c", "mklink", "/J", str(link), str(target)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return {
        "created": completed.returncode == 0 and link.exists(),
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
        "is_junction": bool(getattr(link, "is_junction", lambda: False)()) if link.exists() else False,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="localbridge-lb000-mcp-") as temp:
        root = Path(temp)
        workspace = root / "workspace"
        outside = root / "outside"
        workspace.mkdir()
        outside.mkdir()
        (workspace / "inside.txt").write_text("LB000_INSIDE\n", encoding="utf-8")
        (workspace / "mutable.txt").write_text("before\n", encoding="utf-8")
        (outside / "secret.txt").write_text("LB000_OUTSIDE_SECRET\n", encoding="utf-8")

        junction = workspace / "junction-out"
        junction_result = create_junction(junction, outside)
        symlink = workspace / "symlink-out"
        symlink_result: dict[str, Any]
        try:
            os.symlink(outside, symlink, target_is_directory=True)
            symlink_result = {"created": True, "error": None}
        except OSError as exc:
            symlink_result = {"created": False, "error": f"{type(exc).__name__}: {exc}"}

        safe_process, safe_client, initialize, safe_tools = run_server(workspace, "safe")
        safe_log: dict[str, Any] = {}
        try:
            unauthenticated_error: str | None = None
            try:
                McpClient(safe_client.endpoint).initialize()
            except RuntimeError as exc:
                unauthenticated_error = str(exc)
            server_info = safe_client.call("server_info", {})
            inside_read = safe_client.call("read_file", {"path": "inside.txt"})
            traversal_read = safe_client.call("read_file", {"path": "../outside/secret.txt"})
            junction_read = (
                safe_client.call("read_file", {"path": "junction-out/secret.txt"}) if junction_result["created"] else None
            )
            symlink_read = (
                safe_client.call("read_file", {"path": "symlink-out/secret.txt"}) if symlink_result["created"] else None
            )
            exec_result = safe_client.call(
                "exec_command",
                {
                    "cmd": "cmd.exe /d /c echo LB000_EXEC",
                    "timeout_ms": 5000,
                    "yield_time_ms": 5000,
                    "max_output_bytes": 10000,
                },
            )
            patch_result = safe_client.call(
                "apply_patch",
                {
                    "patch": "*** Begin Patch\n*** Update File: mutable.txt\n@@\n-before\n+after\n*** End Patch\n"
                },
            )
            unknown_error: str | None = None
            try:
                safe_client.call("definitely_unknown_lb000_tool", {})
            except RuntimeError as exc:
                unknown_error = str(exc)
        finally:
            safe_log = stop_server(safe_process)

        trusted_process, _, trusted_initialize, trusted_tools = run_server(workspace, "trusted")
        try:
            pass
        finally:
            trusted_log = stop_server(trusted_process)

        safe_names = [str(tool.get("name")) for tool in safe_tools]
        trusted_names = [str(tool.get("name")) for tool in trusted_tools]
        safe_hash = canonical_hash(safe_tools)
        trusted_hash = canonical_hash(trusted_tools)

        tools_snapshot = {
            "schema_version": 1,
            "source": "live HTTP MCP tools/list",
            "runtime": "coding-tools-mcp",
            "runtime_version": initialize.get("serverInfo", {}).get("version"),
            "protocol_version": initialize.get("protocolVersion"),
            "permission_mode": "safe",
            "telemetry_forced_off": True,
            "tools_sha256": safe_hash,
            "tool_count": len(safe_tools),
            "tools": safe_tools,
        }
        behavior = {
            "schema_version": 1,
            "runtime_version": initialize.get("serverInfo", {}).get("version"),
            "safe_tool_names": safe_names,
            "trusted_tool_names": trusted_names,
            "safe_and_trusted_catalog_identical": safe_names == trusted_names and safe_hash == trusted_hash,
            "safe_tools_sha256": safe_hash,
            "trusted_tools_sha256": trusted_hash,
            "server_info": structured(server_info),
            "bearer_auth_enforced": bool(unauthenticated_error and "HTTP 401" in unauthenticated_error),
            "unauthenticated_error": unauthenticated_error,
            "authenticated_session_id_issued": safe_client.session_id is not None,
            "inside_read_ok": not is_error(inside_read) and "LB000_INSIDE" in json.dumps(inside_read),
            "traversal_denied": is_error(traversal_read),
            "junction_denied": None if junction_read is None else is_error(junction_read),
            "symlink_denied": None if symlink_read is None else is_error(symlink_read),
            "safe_mode_exec_present": "exec_command" in safe_names,
            "safe_mode_exec_succeeded": not is_error(exec_result) and "LB000_EXEC" in json.dumps(exec_result),
            "safe_mode_patch_present": "apply_patch" in safe_names,
            "safe_mode_patch_succeeded": not is_error(patch_result) and (workspace / "mutable.txt").read_text(encoding="utf-8") == "after\n",
            "unknown_tool_fail_closed": unknown_error is not None,
            "unknown_tool_error": unknown_error,
            "safe_server_log": safe_log,
            "trusted_server_log": trusted_log,
            "trusted_initialize": trusted_initialize,
        }
        path_probe = {
            "schema_version": 1,
            "junction": junction_result,
            "symlink": symlink_result,
            "traversal_denied": behavior["traversal_denied"],
            "junction_denied": behavior["junction_denied"],
            "symlink_denied": behavior["symlink_denied"],
            "outside_secret_leaked": "LB000_OUTSIDE_SECRET" in json.dumps([traversal_read, junction_read, symlink_read]),
        }
        auth_transport = {
            "schema_version": 1,
            "transport": "streamable_http",
            "endpoint_path": "/mcp",
            "bind": "127.0.0.1:ephemeral",
            "protocol_version": initialize.get("protocolVersion"),
            "auth": "bearer",
            "auth_secret_injection": "child_environment:CODING_TOOLS_MCP_AUTH_TOKEN",
            "unauthenticated_initialize_rejected": behavior["bearer_auth_enforced"],
            "mcp_session_id_issued": behavior["authenticated_session_id_issued"],
            "session_header_reused_for_tools_calls": behavior["authenticated_session_id_issued"],
            "stdio_supported_but_not_selected_for_localbridge_http_adapter": True,
            "oauth_mode_supported_but_not_selected_for_guard_to_upstream_hop": True,
        }

        (output_dir / "tools-list.json").write_text(
            json.dumps(tools_snapshot, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        (output_dir / "behavior-snapshot.json").write_text(
            json.dumps(behavior, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        (output_dir / "path-probe.json").write_text(
            json.dumps(path_probe, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        (output_dir / "auth-transport.json").write_text(
            json.dumps(auth_transport, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )

        required = [
            tools_snapshot["runtime_version"] == "0.2.2",
            behavior["safe_and_trusted_catalog_identical"],
            behavior["inside_read_ok"],
            behavior["traversal_denied"],
            behavior["junction_denied"] is True,
            not path_probe["outside_secret_leaked"],
            behavior["safe_mode_exec_present"],
            behavior["safe_mode_exec_succeeded"],
            behavior["unknown_tool_fail_closed"],
            behavior["bearer_auth_enforced"],
            behavior["authenticated_session_id_issued"],
        ]
        if symlink_result["created"]:
            required.append(behavior["symlink_denied"] is True)
        ok = all(required)
        print(json.dumps({"ok": ok, "output_dir": str(output_dir), "behavior": behavior, "path_probe": path_probe}, ensure_ascii=True))
        return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
