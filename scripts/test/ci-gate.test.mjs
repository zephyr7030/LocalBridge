import assert from "node:assert/strict";
import test from "node:test";

import { CI_STAGES, parseGateArguments } from "./ci-gate.mjs";
import { selectStages, validateStages } from "./process.mjs";

test("the shared local and CI gate has stable unique stages", () => {
  assert.deepEqual(
    CI_STAGES.map((stage) => stage.id),
    [
      "test-base",
      "format",
      "public-release",
      "licenses",
      "schema44",
      "frontend-test",
      "frontend-build",
      "runtime-resources",
      "rust-test",
      "rust-clippy",
      "nsis-package",
    ],
  );
  assert.throws(
    () => validateStages([CI_STAGES[0], CI_STAGES[0]]),
    /duplicate test stage id/,
  );
});

test("targeted local diagnosis reuses the declared gate instead of copying commands", () => {
  assert.deepEqual(selectStages(CI_STAGES, { only: "rust-test" }).map(({ id }) => id), [
    "rust-test",
  ]);
  assert.deepEqual(selectStages(CI_STAGES, { from: "rust-clippy" }).map(({ id }) => id), [
    "rust-clippy",
    "nsis-package",
  ]);
  assert.deepEqual(parseGateArguments(["--only", "schema44"]), {
    only: "schema44",
  });
  assert.throws(() => parseGateArguments(["--only", "format", "--from", "rust-test"]));
});
