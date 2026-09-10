import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { ActivityDetail } from "../features/activity/ActivityDetail";
import type { ActivityEntry } from "../features/activity/api";

it("renders activity detail in the shared sheet contract with right-aligned actions", () => {
  const entry: ActivityEntry = {
    source: "tool",
    timestampMs: 1,
    action: "filesystem",
    operation: "delete",
    target: "D:\\project\\LocalBridge\\temp",
    outcome: "failed",
    errorCode: "Denied",
    requestId: "req-1",
    connectionId: "conn-1",
    attempt: 1,
    phase: "policy",
    cause: "workspace_denied",
    httpStatus: 403,
    workdir: null,
    durationMs: 318,
    exitCode: null,
    risk: [],
  };
  const rendered = renderToStaticMarkup(<ActivityDetail entry={entry} onClose={() => undefined} />);
  expect(rendered).toContain('data-modal-priority="16"');
  expect(rendered).toContain('class="sheet-scroll"');
  expect(rendered).toContain('class="dialog-actions sheet-actions"');
  expect(rendered).toContain("工作区拒绝");
  expect(rendered).toContain("workspace_denied");
  expect(rendered).toContain("req-1");
  expect(rendered).toContain("完成");
});
