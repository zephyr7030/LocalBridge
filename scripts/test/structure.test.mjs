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

test("black-box scenarios own no private copy of the command terminal driver", () => {
  const source = readFileSync(revisionScenarioPath, "utf8");
  assert.match(source, /from "\.\/command_lifecycle\.mjs"/);
  assert.equal(source.includes("function pollToTerminal"), false);
  // Adding a regression scenario must not require changing a magic call count.
  // Terminal/deadline behavior is asserted by command_lifecycle.test.mjs.
  assert.match(source, /await settleAcceptedPublicCommand\(\{/);
  // 这条只证明场景文件没有自己另写一个轮询器，并且确实用过共享的那个。
  // 它无法证明每一个待决响应都走了共享驱动——那需要源码文本匹配，而这个
  // 仓库已经成建制地退役了那类契约。真正漏用的后果由 CI 上的场景本身暴露：
  // 一个没被驱动到终态的命令会以 status:"running" 直接撞上终态断言。
});
