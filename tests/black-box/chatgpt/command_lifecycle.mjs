const DEFAULT_TERMINAL_DEADLINE_MS = 60_000;
const DEFAULT_POLL_WAIT_MS = 100;

export function publicCommandIsPending(response) {
  const structured = response?.body?.result?.structuredContent;
  return (
    structured?.error?.code === "OperationTimedOut" || structured?.data?.status === "running"
  );
}

// 待决响应有两种形状，session_id 的位置也就有两处：
//   status:"running"  —— 成功响应，id 在 data.session_id
//   OperationTimedOut —— 错误响应，契约规定 data 为 null，id 在 error.details
// 这个断言本身不变：待决就必须给得出一个可 poll 的 id，给不出就是缺陷。
export function pendingPublicSessionId(response) {
  const structured = response?.body?.result?.structuredContent;
  return structured?.data?.session_id ?? structured?.error?.details?.session_id ?? null;
}

export async function settleAcceptedPublicCommand({ initialResponse, ...driver }) {
  if (!publicCommandIsPending(initialResponse)) return initialResponse;
  const publicSessionId = driver.publicSessionId ?? pendingPublicSessionId(initialResponse);
  if (!publicSessionId) {
    const error = new Error("pending public command response has no stable session_id");
    error.code = "PublicSessionIdMissing";
    error.response = initialResponse;
    throw error;
  }
  return drivePublicCommandToTerminal({ ...driver, publicSessionId });
}

export async function drivePublicCommandToTerminal({
  callTool,
  publicSessionId,
  requestPrefix,
  deadlineMs = DEFAULT_TERMINAL_DEADLINE_MS,
  waitMs = DEFAULT_POLL_WAIT_MS,
  monotonicNow = () => performance.now(),
}) {
  if (typeof callTool !== "function") throw new TypeError("callTool must be a function");
  if (!publicSessionId) throw new TypeError("publicSessionId is required");
  if (!requestPrefix) throw new TypeError("requestPrefix is required");
  if (!Number.isFinite(deadlineMs) || deadlineMs <= 0) {
    throw new RangeError("deadlineMs must be positive");
  }

  const deadline = monotonicNow() + deadlineMs;
  let attempt = 0;
  let response;
  do {
    response = await callTool(
      "command_control",
      { action: "poll", session_id: publicSessionId, wait_ms: waitMs },
      `${requestPrefix}-${attempt}`,
    );
    attempt += 1;
    if (!publicCommandIsPending(response)) return response;
  } while (monotonicNow() < deadline);

  const error = new Error(`public command did not become terminal before ${deadlineMs}ms`);
  error.code = "CommandTerminalDeadlineExceeded";
  error.lastResponse = response;
  throw error;
}
