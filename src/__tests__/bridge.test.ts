import { describe, expect, it } from "vitest";
import { uiErrorMessage, type UiError } from "../bridge";

describe("typed UI error boundary", () => {
  it("renders the message from the structured backend error", () => {
    const error: UiError = {
      code: "Scheduler.QueueCapacityExceeded",
      category: "capacity",
      message: "queue is full",
      retryable: true,
      operationId: "operation-1",
      sessionId: "session-1",
      requestId: 7,
      taskId: "task-1",
    };

    expect(uiErrorMessage(error, "fallback")).toBe("queue is full");
  });

  it("does not treat an untyped string as the UI error contract", () => {
    expect(uiErrorMessage("legacy string error", "fallback")).toBe("fallback");
  });
});
