import { pathToFileURL } from "node:url";

import { runStages, selectStages, validateStages } from "./process.mjs";

const cargo = (...args) => ({ program: "cargo", args: ["+1.85.0", ...args] });
const node = (...args) => ({ program: process.execPath, args });

export const CI_STAGES = validateStages([
  {
    id: "test-base",
    label: "test infrastructure contract",
    ...node("--test", "scripts/test/ci-gate.test.mjs"),
  },
  {
    id: "format",
    label: "public source formatting",
    ...node("scripts/public-release/preflight.mjs", "format-check"),
  },
  {
    id: "public-release",
    label: "public export policy",
    ...node("tests/integration/release-preflight/public_release.test.mjs"),
  },
  {
    id: "licenses",
    label: "dependency license policy",
    ...node("scripts/public-release/preflight.mjs", "verify-license"),
  },
  {
    id: "schema44",
    label: "schema44 architecture invariants",
    ...node("scripts/verify-schema44/index.mjs"),
  },
  {
    id: "frontend-test",
    label: "frontend unit tests",
    program: "npm",
    args: ["test"],
  },
  {
    id: "frontend-build",
    label: "frontend typecheck and build",
    program: "npm",
    args: ["run", "build"],
  },
  {
    id: "runtime-resources",
    label: "pinned runtime resources",
    ...node("scripts/prepare-lb018-resources.mjs"),
  },
  {
    id: "rust-test",
    label: "Rust unit and integration tests",
    ...cargo("test", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--", "--test-threads=1"),
  },
  {
    id: "rust-clippy",
    label: "Rust lint gate",
    ...cargo(
      "clippy",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--locked",
      "--all-targets",
      "--",
      "-D",
      "warnings",
    ),
  },
  {
    id: "nsis-package",
    label: "production NSIS package",
    ...node("node_modules/@tauri-apps/cli/tauri.js", "build", "--bundles", "nsis"),
  },
]);

export function parseGateArguments(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--list") options.list = true;
    else if (argument === "--only") options.only = args[++index];
    else if (argument === "--from") options.from = args[++index];
    else throw new Error(`unknown test gate argument: ${argument}`);
  }
  if (options.only && options.from) throw new Error("--only and --from are mutually exclusive");
  return options;
}

export function main(args = process.argv.slice(2)) {
  const options = parseGateArguments(args);
  const selected = selectStages(CI_STAGES, options);
  if (options.list) {
    for (const stage of selected) process.stdout.write(`${stage.id}\t${stage.label}\n`);
    return;
  }
  runStages(selected);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
