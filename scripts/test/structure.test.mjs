import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { extname, join, relative } from "node:path";
import test from "node:test";

import { repositoryRoot } from "./process.mjs";

function filesBelow(root) {
  const files = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) files.push(...filesBelow(path));
    else files.push(path);
  }
  return files;
}

const testScripts = filesBelow(join(repositoryRoot, "tests")).filter((path) =>
  [".mjs", ".js", ".cjs"].includes(extname(path)),
);
const chatGptClientPath = join(
  repositoryRoot,
  "tests",
  "black-box",
  "chatgpt",
  "client.mjs",
);
const revisionScenarioPath = join(
  repositoryRoot,
  "tests",
  "black-box",
  "chatgpt",
  "revision46.mjs",
);

test("legacy governance and schema-generation scripts cannot return to the live test tree", () => {
  const forbiddenNames = testScripts
    .map((path) => relative(repositoryRoot, path).replaceAll("\\", "/"))
    .filter((path) => /(?:^|\/)ARCH-|schema(?:39|40|41|42|43)[^/]*\.test\.mjs$/i.test(path));
  assert.deepEqual(forbiddenNames, []);
});

test("test scripts do not recursively launch the shared Rust or frontend gates", () => {
  const recursiveRunners = [];
  for (const path of testScripts) {
    const source = readFileSync(path, "utf8");
    if (
      /(?:spawnSync|execFileSync|execSync)\s*\(\s*["'](?:cargo|npm|npm\.cmd)["']/s.test(source)
    ) {
      recursiveRunners.push(relative(repositoryRoot, path).replaceAll("\\", "/"));
    }
  }
  assert.deepEqual(recursiveRunners, []);
});

test("the ChatGPT simulator remains outside production and internal state boundaries", () => {
  const source = readFileSync(chatGptClientPath, "utf8");
  for (const forbidden of [
    "src-tauri",
    "@tauri-apps",
    "AgentFacade",
    "TaskRegistry",
    "ExecutionRegistry",
    "invoke(",
  ]) {
    assert.equal(source.includes(forbidden), false, `client imports internal seam: ${forbidden}`);
  }
});

test("black-box scenarios reuse the shared command terminal driver", () => {
  const source = readFileSync(revisionScenarioPath, "utf8");
  assert.match(source, /from "\.\/command_lifecycle\.mjs"/);
  assert.equal(source.includes("function pollToTerminal"), false);
  // Adding a regression scenario must not require changing a magic call count.
  // Terminal/deadline behavior is asserted by command_lifecycle.test.mjs.
  assert.match(source, /await settleAcceptedPublicCommand\(\{/);
});
