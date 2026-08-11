import assert from "node:assert/strict";
import { classifyArchitectureRules } from "./core.mjs";

const supported = new Set(["task_summary_redaction"]);
const rule = (verification) => ({ id: "ARCH-011", description: "fixture", verification });
const progress = (status) => ({
  execution: { execution_order: ["LB-001", "LB-002", "LB-003"] },
  prs: [
    { id: "LB-001", status: "PASS" },
    { id: "LB-002", status },
    { id: "LB-003", status: "BLOCKED" },
  ],
});
const declared = {
  mode: "deferred",
  activate_at_pr: "LB-002",
  reason: "fixture activation",
  type: "task_summary_redaction",
};

for (const status of ["IN_PROGRESS", "PASS", "FAILED", "REWORK_REQUIRED"]) {
  const result = classifyArchitectureRules({ rules: [rule(declared)] }, progress(status), supported);
  assert.deepEqual(result.activatedDeferred.map((item) => item.id), ["ARCH-011"]);
  assert.equal(result.futureDeferred.length, 0);
}
for (const status of ["READY", "BLOCKED"]) {
  const result = classifyArchitectureRules({ rules: [rule(declared)] }, progress(status), supported);
  assert.deepEqual(result.futureDeferred.map((item) => item.id), ["ARCH-011"]);
  assert.equal(result.activatedDeferred.length, 0);
}
const prScoped = classifyArchitectureRules(
  { rules: [rule({ mode: "deferred", activate_at_pr: "LB-002", reason: "fixture activation" })] },
  progress("PASS"),
  supported,
);
assert.equal(prScoped.activatedDeferred.length, 1);
assert.throws(
  () => classifyArchitectureRules(
    { rules: [rule({ ...declared, activate_at_pr: "LB-099" })] },
    progress("PASS"),
    supported,
  ),
  /activate_at_pr is not in execution_order/,
);
console.log("ARCHITECTURE_ACTIVATION_TEST=PASS active_statuses=4 future_statuses=2 pr_scoped_resolution_delegated=true");
