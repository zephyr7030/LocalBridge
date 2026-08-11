from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


def load_json(relative: str):
    return json.loads((ROOT / relative).read_text(encoding="utf-8-sig"))


class LB000EvidenceTests(unittest.TestCase):
    def test_real_tools_list_and_structural_baseline(self):
        expected = load_json("tests/fixtures/upstream/lb000_expected.json")["coding_tools_mcp"]
        snapshot = load_json("compatibility/coding-tools/0.2.2/tools-list.json")
        structural = load_json("compatibility/coding-tools/0.2.2/structural-baseline.json")
        structural_diff = load_json("compatibility/coding-tools/0.2.2/structural-diff.json")
        capability_diff = load_json("compatibility/coding-tools/0.2.2/capability-diff.json")
        self.assertEqual(snapshot["runtime_version"], expected["version"])
        self.assertEqual(snapshot["tool_count"], expected["tool_count"])
        self.assertEqual(snapshot["tools_sha256"], expected["tools_list_sha256"])
        self.assertEqual(structural["commit"], expected["commit"])
        self.assertEqual(structural["tools_list_sha256"], expected["tools_list_sha256"])
        self.assertEqual(structural_diff["comparison_kind"], "initial_baseline")
        self.assertEqual(structural_diff["current"]["tools_list_sha256"], expected["tools_list_sha256"])
        self.assertEqual(capability_diff["comparison_kind"], "initial_baseline")
        self.assertEqual(capability_diff["decision"], "freeze_as_initial_capability_baseline")

    def test_capability_map_covers_exact_catalog_and_enforces_edit(self):
        snapshot = load_json("compatibility/coding-tools/0.2.2/tools-list.json")
        capability = load_json("compatibility/coding-tools/0.2.2/capability-map.json")
        names = {tool["name"] for tool in snapshot["tools"]}
        self.assertEqual(names, set(capability["tools"]))
        self.assertEqual(capability["tools"]["exec_command"]["edit"], "deny")
        self.assertEqual(capability["tools"]["request_permissions"]["edit"], "deny")
        self.assertEqual(capability["tools"]["request_permissions"]["full"], "deny")
        self.assertEqual(capability["native_workflow_tools"], [])
        self.assertEqual(capability["unknown_tool_policy"], "deny")

    def test_real_mcp_behavior_and_reparse_adversarial_probe(self):
        behavior = load_json("compatibility/coding-tools/0.2.2/behavior-snapshot.json")
        path_probe = load_json("compatibility/coding-tools/0.2.2/path-probe.json")
        auth_transport = load_json("compatibility/coding-tools/0.2.2/auth-transport.json")
        self.assertTrue(behavior["safe_and_trusted_catalog_identical"])
        self.assertTrue(behavior["bearer_auth_enforced"])
        self.assertTrue(behavior["authenticated_session_id_issued"])
        self.assertTrue(auth_transport["unauthenticated_initialize_rejected"])
        self.assertTrue(auth_transport["mcp_session_id_issued"])
        self.assertEqual(auth_transport["auth"], "bearer")
        self.assertEqual(auth_transport["bind"], "127.0.0.1:ephemeral")
        self.assertTrue(behavior["safe_mode_exec_present"])
        self.assertTrue(behavior["safe_mode_exec_succeeded"])
        self.assertTrue(behavior["unknown_tool_fail_closed"])
        self.assertTrue(path_probe["junction"]["created"])
        self.assertTrue(path_probe["junction"]["is_junction"])
        self.assertTrue(path_probe["symlink"]["created"])
        self.assertTrue(path_probe["traversal_denied"])
        self.assertTrue(path_probe["junction_denied"])
        self.assertTrue(path_probe["symlink_denied"])
        self.assertFalse(path_probe["outside_secret_leaked"])

    def test_job_object_poc(self):
        result = load_json("spikes/lb-000/job-object-result.json")
        self.assertTrue(result["ok"])
        self.assertTrue(result["before_job_close"]["root_alive"])
        self.assertTrue(result["before_job_close"]["nested_alive"])
        self.assertFalse(result["after_job_close"]["root_alive"])
        self.assertFalse(result["after_job_close"]["nested_alive"])
        self.assertEqual(result["ownership_model"], "kernel job handle, not PID-only")

    def test_embedded_python_poc(self):
        expected = load_json("tests/fixtures/upstream/lb000_expected.json")["python_feasibility"]
        result = load_json("spikes/lb-000/portable-python-result.json")
        self.assertTrue(result["ok"])
        self.assertEqual(result["sample_python_version"], expected["sample_version"])
        self.assertEqual(result["archive_sha256"], expected["archive_sha256"])
        self.assertTrue(result["runtime_execution_used_embedded_python_directly"])
        self.assertFalse(result["runtime_pip_install"])
        self.assertEqual(result["actual_http_mcp_probe_exit"], 0)

    def test_tunnel_asset_secret_injection_and_readiness_semantics(self):
        expected = load_json("tests/fixtures/upstream/lb000_expected.json")["tunnel_client"]
        asset = load_json("compatibility/tunnel-client/0.0.11/release-asset.json")
        result = load_json("spikes/lb-000/tunnel-probe-result.json")
        health = load_json("compatibility/tunnel-client/0.0.11/health-contract.json")
        self.assertEqual(asset["git_commit"], expected["commit"])
        self.assertEqual(asset["official_windows_amd64_asset"], expected["windows_asset"])
        self.assertEqual(asset["asset_sha256"], expected["windows_asset_sha256"])
        self.assertTrue(result["ok"])
        self.assertFalse(result["argv_contains_secret"])
        self.assertFalse(result["os_command_line_contains_secret"])
        self.assertTrue(result["os_command_line_uses_env_reference"])
        self.assertTrue(result["health_loopback"])
        self.assertEqual(result["ready_status"], 200)
        self.assertTrue(result["admin_status"].get("tunnel_metadata_error"))
        self.assertIn("/readyz alone MUST NOT project Ready", health["localbridge_ready_rule"])

    def test_tunnel_cli_raw_snapshot_contains_required_contract(self):
        cli = load_json("compatibility/tunnel-client/0.0.11/cli-surface.json")
        raw = (ROOT / cli["raw_run_help_file"]).read_text(encoding="utf-8-sig")
        for flag in cli["required_localbridge_flags_verified"]:
            self.assertIn(flag, raw)
        self.assertIn("env:VARNAME", raw)
        self.assertIn("127.0.0.1:0", raw)

    def test_runtime_policy_is_guarded_and_telemetry_off(self):
        policy = tomllib.loads((ROOT / "runtime-policy.toml").read_text(encoding="utf-8"))
        self.assertEqual(policy["enforcement"]["implementation"], "first_party_rust_mcp_guard")
        self.assertEqual(policy["enforcement"]["tools_call_check"], "mandatory")
        self.assertEqual(policy["capabilities"]["unknown"], "deny")
        self.assertNotIn("exec_command", policy["edit_allowed_tools"])
        self.assertIn("exec_command", policy["full_allowed_tools"])
        self.assertIn("request_permissions", policy["blocked_tools"])
        self.assertEqual(policy["upstream_coding_tools"]["telemetry"], "disabled")

    def test_baseline_and_adrs_exist(self):
        baseline = load_json("COMPATIBILITY_BASELINE.json")
        self.assertEqual(baseline["coding_tools_mcp"]["version"], "0.2.2")
        self.assertEqual(baseline["tunnel_client"]["version"], "0.0.11")
        self.assertEqual(baseline["policy_enforcement"]["decision"], "first_party_rust_mcp_guard")
        self.assertEqual(baseline["process_ownership"]["decision"], "windows_job_object")
        for path in [
            "docs/UPSTREAM_COMPAT_REPORT.md",
            "docs/ADR-0001_POLICY_ENFORCEMENT.md",
            "docs/ADR-0002_PROCESS_OWNERSHIP.md",
        ]:
            self.assertTrue((ROOT / path).is_file(), path)

    def test_live_tunnel_gate_and_acceptance(self):
        live = load_json("spikes/lb-000/live-tunnel-result.json")
        acceptance = load_json("compatibility/lb-000-acceptance.json")
        self.assertEqual(live["schema_version"], 2)
        self.assertEqual(live["status"], "PASS")
        self.assertTrue(live["authenticated_control_plane_metadata_observed"])
        self.assertTrue(live["control_plane_poll_success_observed"])
        self.assertTrue(live["control_plane_poll_last_successful_timestamp_present"])
        self.assertGreaterEqual(live["control_plane_poll_cycles_observed"], 1)
        self.assertTrue(live["tunnel_working_state_observed"])
        self.assertEqual(live["ready_status"], 200)
        self.assertTrue(live["os_command_line_observed"])
        self.assertFalse(live["api_key_in_command_line"])
        self.assertTrue(live["api_key_reference_in_command_line"])
        self.assertFalse(live["tunnel_id_in_command_line"])
        self.assertFalse(live["stdout_contains_api_key"])
        self.assertFalse(live["stderr_contains_api_key"])
        self.assertFalse(live["secrets_emitted"])
        self.assertEqual(live["credential_input"], "stdin_to_child_environment")
        self.assertTrue(live["health_loopback"])
        self.assertEqual(acceptance["status"], "PASS")
        self.assertEqual(acceptance["external_gates"]["live_tunnel_poc"]["status"], "PASS")
        self.assertTrue(acceptance["external_gates"]["live_tunnel_poc"]["control_plane_poll_success_observed"])
        self.assertTrue(acceptance["external_gates"]["live_tunnel_poc"]["tunnel_working_state_observed"])
        self.assertTrue(acceptance["external_gates"]["live_tunnel_poc"]["measurements_are_runtime_observed"])
        self.assertTrue(acceptance["governance_transition_allowed"])
        self.assertFalse(acceptance["next_pr_unlocked"])


if __name__ == "__main__":
    unittest.main()
