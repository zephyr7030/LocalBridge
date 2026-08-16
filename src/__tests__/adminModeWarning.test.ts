import { describe, expect, it } from "vitest";
import { adminWarningCanConfirm, adminWarningRemainingSeconds } from "../components/AdminModeWarning";

describe("LB-015 administrator warning monotonic gate", () => {
  it("holds the confirmation gate for the full 9000ms", () => {
    const start = 1000;
    expect(adminWarningCanConfirm(start, start)).toBe(false);
    expect(adminWarningCanConfirm(start, start + 8999.999)).toBe(false);
    expect(adminWarningCanConfirm(start, start + 9000)).toBe(true);
    expect(adminWarningCanConfirm(start, start + 12000)).toBe(true);
  });

  it("derives confirmation countdown labels from elapsed monotonic time", () => {
    const start = 500;
    for (let second = 0; second < 9; second += 1) {
      expect(adminWarningRemainingSeconds(start, start + second * 1000)).toBe(9 - second);
    }
    expect(adminWarningRemainingSeconds(start, start + 8999)).toBe(1);
    expect(adminWarningRemainingSeconds(start, start + 9000)).toBe(0);
  });
});
