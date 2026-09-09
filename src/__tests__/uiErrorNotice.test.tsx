import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { UiErrorNotice } from "../components/UiErrorNotice";

const error = {
  code: "Settings.SaveFailed", category: "internal" as const, message: "Cannot save", retryable: true,
  operationId: "op-123", sessionId: "session-456", requestId: 789, taskId: "task-012",
};

it("keeps technical backend details out of normal UI errors", () => {
  const rendered = renderToStaticMarkup(<UiErrorNotice error={error} />);
  expect(rendered).toContain('role="alert"');
  expect(rendered).toContain("<span>操作未完成</span>");
  for (const technical of ["Cannot save", "Settings.SaveFailed", "op-123", "session-456", "789", "task-012"]) {
    expect(rendered).not.toContain(technical);
  }
  expect(rendered).not.toContain("<details>");
});

it("retains the full backend envelope when diagnostics explicitly asks for it", () => {
  const rendered = renderToStaticMarkup(<UiErrorNotice error={error} showTechnicalDetails />);
  expect(rendered).toContain("<span>操作未完成</span>");
  for (const fact of ["Cannot save", "Settings.SaveFailed", "internal", "retryable", "op-123", "session-456", "789", "task-012"]) {
    expect(rendered).toContain(fact);
  }
  expect(rendered).toContain("<details>");
});
