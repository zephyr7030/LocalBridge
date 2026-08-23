import assert from "node:assert/strict";
import test from "node:test";

import { CI_STAGES, cargoCommand, parseGateArguments } from "./ci-gate.mjs";
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
  assert.deepEqual(
    selectStages(CI_STAGES, { from: "schema44", through: "frontend-build" }).map(({ id }) => id),
    ["schema44", "frontend-test", "frontend-build"],
  );
  assert.deepEqual(
    selectStages(CI_STAGES, { through: "rust-clippy" }).at(-1)?.id,
    "rust-clippy",
  );
  assert.deepEqual(parseGateArguments(["--only", "schema44"]), {
    only: "schema44",
  });
  assert.deepEqual(parseGateArguments(["--from", "schema44", "--through", "rust-test"]), {
    from: "schema44",
    through: "rust-test",
  });
  assert.throws(() => parseGateArguments(["--only", "format", "--from", "rust-test"]));
  assert.throws(() => selectStages(CI_STAGES, { from: "rust-test", through: "format" }));
});

test("a running desktop binary can use the same Rust gate with an isolated target directory", () => {
  assert.deepEqual(cargoCommand(["test"], {}).args, ["+1.85.0", "test"]);
  assert.deepEqual(
    cargoCommand(["test"], { LOCALBRIDGE_CARGO_TARGET_DIR: "src-tauri/target/local-gate" }).args,
    ["+1.85.0", "test", "--target-dir", "src-tauri/target/local-gate"],
  );
});
