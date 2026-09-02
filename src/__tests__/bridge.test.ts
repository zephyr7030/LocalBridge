import { describe, expect, it } from "vitest";
import { parseMainProjection, parseUiError, uiErrorMessage, type UiError } from "../bridge";
import { projectionReadFailed, projectionReadSucceeded } from "../projectionTransport";
import { parseDiagnosticsProjection } from "../features/diagnostics/api";
import { parseOnboardingState } from "../features/onboarding/api";
import mainProjectionFixture from "../../tests/fixtures/ui/main_projection.json";

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
    expect(parseUiError(error, "fallback")).toEqual(error);
  });

  it("does not treat an untyped string as the UI error contract", () => {
    expect(uiErrorMessage("legacy string error", "fallback")).toBe("fallback");
  });
});

describe("frontend projection freshness", () => {
  it("discards the previous successful projection after any transport or parse failure", () => {
    const projection = parseMainProjection(mainProjectionFixture);
    const fresh = projectionReadSucceeded(projection);
    expect(fresh.projection?.projectionRevision).toBe(projection.projectionRevision);
    const unavailable = projectionReadFailed(parseUiError({
      code: "Ui.BackendUnavailable",
      category: "unavailable",
      message: "backend unavailable",
      retryable: true,
      operationId: "operation-9",
      sessionId: "session-9",
      requestId: 9,
      taskId: "task-9",
    }, "fallback"));
    expect(unavailable.freshness).toBe("unavailable");
    expect(unavailable.projection).toBeNull();
    expect(unavailable.error).toMatchObject({
      code: "Ui.BackendUnavailable",
      operationId: "operation-9",
      sessionId: "session-9",
      requestId: 9,
      taskId: "task-9",
    });
  });
});

describe("backend projection contract", () => {
  it("parses the Rust-owned JSON fixture without losing lifecycle states", () => {
    const projection = parseMainProjection(mainProjectionFixture);
    expect(projection.permissionReconciliation).toBe("awaiting_authorization");
    expect(projection.workspace?.effective).toBe("available");
    expect(projection.connection?.observedTunnelId).toBe(projection.connection?.desiredTunnelId);
    expect(projection.currentActivity?.state).toBe("waiting_input");
    expect(projection.lastActivity?.outcome).toBe("blocked");
    expect(projection.activeFaults).toHaveLength(1);
  });

  it("rejects a partial backend payload instead of silently inventing defaults", () => {
    expect(() => parseMainProjection({ permission: "edit" })).toThrow("后端主状态合同不兼容");
  });
});

describe("feature projection contracts", () => {
  it("validates diagnostics and onboarding responses at the Tauri boundary", () => {
    expect(parseDiagnosticsProjection({
      projectionRevision: 3,
      logRevision: 2,
      checks: [{ code: "runtime", label: "Runtime", level: "unknown", detail: "unavailable" }],
      privilege: "unavailable",
      activeWorkspacePath: null,
      recentEvents: [],
      activeFaults: [{ code: "Runtime.Unavailable", category: "unavailable", message: "Unavailable", retryable: true }],
    }).projectionRevision).toBe(3);
    expect(parseOnboardingState({
      complete: false,
      projectionRevision: 3,
      connectionConfigured: true,
      runtimeKeySaved: true,
      runtimeKeyLength: 51,
      tunnelId: "tunnel_01401401401401401401401401401401",
      readiness: { localEnvironment: true, codingService: true, openaiTunnel: false },
    }).readiness.openaiTunnel).toBe(false);
    expect(() => parseDiagnosticsProjection({ projectionRevision: 3 })).toThrow("后端诊断状态合同不兼容");
    expect(() => parseOnboardingState({ complete: false })).toThrow("后端首次设置状态合同不兼容");
  });
});
