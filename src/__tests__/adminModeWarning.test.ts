import { describe, expect, it } from "vitest";
import { adminWarningCanConfirm, adminWarningRemainingSeconds } from "../components/AdminModeWarning";

describe("LB-015 administrator warning monotonic gate", () => {
  it("uses the backend not-before deadline as the confirmation source", () => {
    const notBefore = 10000;
    expect(adminWarningCanConfirm(notBefore, 1000)).toBe(false);
    expect(adminWarningCanConfirm(notBefore, 9999.999)).toBe(false);
    expect(adminWarningCanConfirm(notBefore, 10000)).toBe(true);
    expect(adminWarningCanConfirm(notBefore, 13000)).toBe(true);
  });

  it("derives visual countdown labels from the backend deadline", () => {
    const notBefore = 3500;
    for (let second = 0; second < 3; second += 1) {
      expect(adminWarningRemainingSeconds(notBefore, 500 + second * 1000)).toBe(3 - second);
    }
    expect(adminWarningRemainingSeconds(notBefore, 3499)).toBe(1);
    expect(adminWarningRemainingSeconds(notBefore, 3500)).toBe(0);
  });
});
