export const ACTIVATED_PR_STATUSES = new Set([
  "IN_PROGRESS",
  "PASS",
  "FAILED",
  "REWORK_REQUIRED",
]);

export const FUTURE_PR_STATUSES = new Set(["READY", "BLOCKED"]);

export function classifyArchitectureRules(rulesDoc, prIndex, supportedTypes) {
  if (!Array.isArray(rulesDoc.rules) || rulesDoc.rules.length === 0) {
    throw new Error("architecture rule inventory is empty");
  }
  const order = prIndex?.execution?.execution_order;
  if (!Array.isArray(order) || order.length === 0) {
    throw new Error("PR progress is missing execution.execution_order");
  }
  if (new Set(order).size !== order.length) {
    throw new Error("PR execution_order contains duplicates");
  }
  const prById = new Map((prIndex.prs ?? []).map((pr) => [pr.id, pr]));
  for (const prId of order) {
    if (!prById.has(prId)) throw new Error(`PR progress missing execution_order entry: ${prId}`);
  }

  const ids = new Set();
  const configuredEnforced = [];
  const activatedDeferred = [];
  const futureDeferred = [];

  for (const rule of rulesDoc.rules) {
    if (!/^ARCH-\d{3}$/.test(rule.id) || ids.has(rule.id)) {
      throw new Error(`invalid or duplicate architecture rule id: ${rule.id}`);
    }
    ids.add(rule.id);
    const verification = rule.verification;
    if (!verification || !["enforced", "deferred"].includes(verification.mode)) {
      throw new Error(`${rule.id} missing supported verification mode`);
    }

    if (verification.mode === "enforced") {
      if (!supportedTypes.has(verification.type)) {
        throw new Error(`${rule.id} has unsupported verifier type: ${verification.type ?? "missing"}`);
      }
      configuredEnforced.push(rule);
      continue;
    }

    const activationPr = verification.activate_at_pr;
    if (!/^LB-\d{3}$/.test(activationPr ?? "") || !verification.reason) {
      throw new Error(`${rule.id} deferred verification requires activate_at_pr and reason`);
    }
    if (!order.includes(activationPr)) {
      throw new Error(`${rule.id} activate_at_pr is not in execution_order: ${activationPr}`);
    }
    const activationStatus = prById.get(activationPr)?.status;
    if (ACTIVATED_PR_STATUSES.has(activationStatus)) {
      if (!supportedTypes.has(verification.type)) {
        throw new Error(`${rule.id} activated at ${activationPr} (${activationStatus}) without a supported verifier type`);
      }
      activatedDeferred.push(rule);
    } else if (FUTURE_PR_STATUSES.has(activationStatus)) {
      futureDeferred.push(rule);
    } else {
      throw new Error(`${rule.id} activation PR ${activationPr} has unsupported status: ${activationStatus ?? "missing"}`);
    }
  }

  return {
    configuredEnforced,
    activatedDeferred,
    futureDeferred,
    activeRules: [...configuredEnforced, ...activatedDeferred],
  };
}
