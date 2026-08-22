import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

export const repositoryRoot = resolve(import.meta.dirname, "..", "..");

const executableFor = (program, args) => {
  if (process.platform === "win32" && program === "npm") {
    return {
      executable: process.env.ComSpec || "cmd.exe",
      args: ["/d", "/s", "/c", "npm.cmd", ...args],
    };
  }
  return { executable: program, args };
};

export function validateStages(stages) {
  if (!Array.isArray(stages) || stages.length === 0) {
    throw new Error("test gate must declare at least one stage");
  }
  const ids = new Set();
  for (const stage of stages) {
    if (!stage?.id || !stage?.program || !Array.isArray(stage.args)) {
      throw new Error(`invalid test stage: ${JSON.stringify(stage)}`);
    }
    if (ids.has(stage.id)) throw new Error(`duplicate test stage id: ${stage.id}`);
    ids.add(stage.id);
  }
  return stages;
}

export function selectStages(stages, { only, from, through } = {}) {
  validateStages(stages);
  if (only) {
    const selected = stages.filter((stage) => stage.id === only);
    if (selected.length === 0) throw new Error(`unknown test stage: ${only}`);
    return selected;
  }
  const start = from ? stages.findIndex((stage) => stage.id === from) : 0;
  if (start < 0) throw new Error(`unknown test stage: ${from}`);
  const end = through ? stages.findIndex((stage) => stage.id === through) : stages.length - 1;
  if (end < 0) throw new Error(`unknown test stage: ${through}`);
  if (start > end) throw new Error(`test stage range is reversed: ${from} through ${through}`);
  return stages.slice(start, end + 1);
}

export function runStage(stage, options = {}) {
  const root = options.root ?? repositoryRoot;
  const started = Date.now();
  const { executable, args } = executableFor(stage.program, stage.args);
  process.stdout.write(`\n[localbridge-test] START ${stage.id}: ${stage.label}\n`);
  const result = spawnSync(executable, args, {
    cwd: stage.cwd ? resolve(root, stage.cwd) : root,
    env: { ...process.env, ...stage.env },
    stdio: "inherit",
    windowsHide: true,
    maxBuffer: 256 * 1024 * 1024,
  });
  const elapsedSeconds = ((Date.now() - started) / 1000).toFixed(1);
  if (result.status !== 0) {
    throw new Error(
      `[localbridge-test] FAIL ${stage.id} after ${elapsedSeconds}s (exit ${result.status ?? "start failure"})${result.error ? `: ${result.error.message}` : ""}`,
    );
  }
  process.stdout.write(`[localbridge-test] PASS ${stage.id} (${elapsedSeconds}s)\n`);
}

export function runStages(stages, options = {}) {
  const selected = selectStages(stages, options);
  const started = Date.now();
  for (const stage of selected) runStage(stage, options);
  process.stdout.write(
    `\n[localbridge-test] COMPLETE ${selected.length} stages (${((Date.now() - started) / 1000).toFixed(1)}s)\n`,
  );
}
