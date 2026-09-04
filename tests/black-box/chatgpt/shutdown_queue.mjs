import assert from "node:assert/strict";
import { ChatGptMcpClient } from "./client.mjs";

const client = new ChatGptMcpClient({
  endpoint: process.argv[2],
  extraHeaders: JSON.parse(process.env.LOCALBRIDGE_TEST_MCP_HEADERS),
  timeoutMs: 30_000,
});
const call = (name, args, requestId) => client.execute({
  op: "tools/call", name, arguments: args, request_id: requestId,
});
await client.connect();
await client.execute({ op: "tools/list" });
// Catch immediately: shutdown may interrupt the active transport. The queued
// request itself must settle as cancelled, never run its filesystem side effect.
const blocker = call("exec_command", {
  command: "Start-Sleep -Seconds 10", shell: "windows_powershell", yield_time_ms: 20_000,
}, "shutdown-blocker").catch((error) => ({ transportError: error.message }));
const waitFor = async (predicate) => {
  const deadline = performance.now() + 5_000;
  while (true) {
    const result = await call("task_control", { action: "get" });
    if (predicate(result.body.result.structuredContent.data.scheduler)) return;
    assert.ok(performance.now() < deadline, "scheduler did not reach the expected state");
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
};
await waitFor((state) => state.foreground_work_running === 1);
const queued = call("exec_command", {
  command: "Set-Content -LiteralPath shutdown-should-not-exist.txt -Value BAD",
  shell: "windows_powershell", yield_time_ms: 10_000,
}, "shutdown-queued");
await waitFor((state) => state.queue_depth === 1);
console.log("SHUTDOWN_QUEUE_READY");
const result = await queued;
assert.equal(result.body.result.structuredContent.error.code, "ProcessCancelled");
await blocker;
console.log("SHUTDOWN_QUEUE_PASS");
