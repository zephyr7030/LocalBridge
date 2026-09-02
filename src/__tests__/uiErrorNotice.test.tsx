import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { UiErrorNotice } from "../components/UiErrorNotice";

it("retains the full backend error envelope in expandable diagnostic details", () => {
  const rendered = renderToStaticMarkup(<UiErrorNotice error={{
    code: "Settings.SaveFailed", category: "internal", message: "Cannot save", retryable: true,
    operationId: "op-123", sessionId: "session-456", requestId: 789, taskId: "task-012",
  }} />);
  expect(rendered).toContain('role="alert"');
  for (const fact of ["Settings.SaveFailed", "internal", "retryable", "op-123", "session-456", "789", "task-012"]) {
    expect(rendered).toContain(fact);
  }
  expect(rendered).toContain("<details>");
});
