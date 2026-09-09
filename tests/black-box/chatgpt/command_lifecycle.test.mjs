import assert from "node:assert/strict";
import test from "node:test";

import {
  callPublicToolUntilAccepted,
  drivePublicCommandToTerminal,
  publicCallWasDeferred,
  publicCommandIsPending,
  settleAcceptedPublicCommand,
} from "./command_lifecycle.mjs";

function response({ status, errorCode, retryable } = {}) {
  return {
    body: {
      result: {
        structuredContent: {
          data: status ? { status } : null,
          error: errorCode ? { code: errorCode, retryable } : null,
        },
      },
    },
  };
}

const busy = () => response({ errorCode: "RuntimeUnavailable", retryable: true });

test("terminal driver treats transport budget expiry and running as non-terminal", async () => {
  const sequence = [
    response({ errorCode: "OperationTimedOut" }),
    response({ status: "running" }),
    response({ status: "cancelled" }),
  ];
  const calls = [];
  const terminal = await drivePublicCommandToTerminal({
    callTool: async (...args) => {
      calls.push(args);
      return sequence.shift();
    },
    publicSessionId: "lb-session-test",
    requestPrefix: "terminal",
  });

  assert.equal(terminal.body.result.structuredContent.data.status, "cancelled");
  assert.equal(calls.length, 3);
  assert.deepEqual(
    calls.map((call) => call[2]),
    ["terminal-0", "terminal-1", "terminal-2"],
  );
  assert.deepEqual(calls[0][1], {
    action: "poll",
    session_id: "lb-session-test",
    wait_ms: 100,
  });
});

test("terminal driver returns typed tool errors instead of retrying them", async () => {
  const denied = response({ errorCode: "SessionUnavailable" });
  assert.equal(publicCommandIsPending(denied), false);
  assert.equal(
    await drivePublicCommandToTerminal({
      callTool: async () => denied,
      publicSessionId: "lb-session-test",
      requestPrefix: "terminal",
    }),
    denied,
  );
});

test("terminal driver has one explicit wall-clock deadline", async () => {
  let now = 0;
  await assert.rejects(
    drivePublicCommandToTerminal({
      callTool: async () => response({ errorCode: "OperationTimedOut" }),
      publicSessionId: "lb-session-test",
      requestPrefix: "terminal",
      deadlineMs: 2,
      monotonicNow: () => now++,
    }),
    { code: "CommandTerminalDeadlineExceeded" },
  );
});

test("accepted command settlement reuses its stable public session identity", async () => {
  const initial = response({ status: "running" });
  initial.body.result.structuredContent.data.session_id = "lb-session-initial";
  const completed = response({ status: "completed" });
  const calls = [];
  assert.equal(
    await settleAcceptedPublicCommand({
      initialResponse: initial,
      callTool: async (...args) => {
        calls.push(args);
        return completed;
      },
      requestPrefix: "settle",
    }),
    completed,
  );
  assert.equal(calls[0][1].session_id, "lb-session-initial");
});

test("only a retryable RuntimeUnavailable is a deferral", () => {
  assert.equal(publicCallWasDeferred(busy()), true);
  assert.equal(
    publicCallWasDeferred(response({ errorCode: "RuntimeUnavailable", retryable: false })),
    false,
    "a missing trusted shell is a verdict, not a busy lane",
  );
  assert.equal(publicCallWasDeferred(response({ errorCode: "InvalidArgument" })), false);
  assert.equal(publicCallWasDeferred(response({ status: "completed" })), false);
});

test("a deferred call is retried under its original request id until the lane runs it", async () => {
  const sequence = [busy(), busy(), response({ errorCode: "InvalidArgument" })];
  const calls = [];
  const accepted = await callPublicToolUntilAccepted({
    callTool: async (...args) => {
      calls.push(args);
      return sequence.shift();
    },
    name: "command_control",
    args: { action: "read", stream: "stderr" },
    requestId: "output-stream-mismatch",
    sleep: async () => {},
  });

  assert.equal(accepted.body.result.structuredContent.error.code, "InvalidArgument");
  assert.equal(calls.length, 3);
  assert.deepEqual(
    calls.map((call) => call[2]),
    ["output-stream-mismatch", "output-stream-mismatch", "output-stream-mismatch"],
  );
});

test("a lane that never frees up has one explicit wall-clock deadline", async () => {
  let now = 0;
  await assert.rejects(
    callPublicToolUntilAccepted({
      callTool: async () => busy(),
      name: "command_control",
      args: {},
      requestId: "never-accepted",
      deadlineMs: 2,
      monotonicNow: () => now++,
      sleep: async () => {},
    }),
    { code: "ControlLaneDeadlineExceeded" },
  );
});
