import assert from "node:assert/strict";
import test from "node:test";

import { CI_STAGES, cargoCommand, parseGateArguments } from "./ci-gate.mjs";
import { selectStages, validateStages } from "./process.mjs";

test("the shared local and CI gate has stable unique stages", () => {
  assert.deepEqual(
    CI_STAGES.map((stage) => stage.id),
    [
      "test-base",
      "toolchain",
      "format",
      "public-release",
      "licenses",
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
    selectStages(CI_STAGES, { from: "frontend-test", through: "frontend-build" }).map(({ id }) => id),
    ["frontend-test", "frontend-build"],
  );
  assert.deepEqual(
    selectStages(CI_STAGES, { through: "rust-clippy" }).at(-1)?.id,
    "rust-clippy",
  );
  assert.deepEqual(parseGateArguments(["--only", "licenses"]), {
    only: "licenses",
  });
  assert.deepEqual(parseGateArguments(["--from", "licenses", "--through", "rust-test"]), {
    from: "licenses",
    through: "rust-test",
  });
  assert.throws(() => parseGateArguments(["--only", "format", "--from", "rust-test"]));
  assert.throws(() => selectStages(CI_STAGES, { from: "rust-test", through: "format" }));
});

test("a running desktop binary can use the same Rust gate with an isolated target directory", () => {
  // 工具链由 rust-toolchain.toml 锁定，而不是在这里再写一遍版本号。
  assert.deepEqual(cargoCommand(["test"], {}).args, ["test"]);
  assert.deepEqual(
    cargoCommand(["test"], { LOCALBRIDGE_CARGO_TARGET_DIR: "src-tauri/target/local-gate" }).args,
    ["test", "--target-dir", "src-tauri/target/local-gate"],
  );
});
