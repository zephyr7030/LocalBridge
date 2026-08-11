import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.env.LOCALBRIDGE_REPO_ROOT;
if (!root || process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-017" || process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-003") process.exit(2);
const registryPath = join(root, "src-tauri", "src", "workspace", "registry.rs");
const migrationPath = join(root, "src-tauri", "src", "settings", "migration.rs");
const testPath = join(root, "tests", "unit", "workspace", "workspace_registry.rs");
if (![registryPath, migrationPath, testPath].every(existsSync)) process.exit(3);
const registry = readFileSync(registryPath, "utf8");
const migration = readFileSync(migrationPath, "utf8");
const tests = readFileSync(testPath, "utf8");
for (const required of [
  "active_workspace_id: Option<WorkspaceId>",
  "remembered_entries",
  "active_entry",
  "validated_identity == incoming.validated_identity",
  "active_workspace_id = None",
]) if (!registry.includes(required)) process.exit(4);
if (/authorized_roots|authorization_roots|display_path[^\n]{0,100}to_(?:ascii_)?lowercase/i.test(registry)) process.exit(5);
if (!migration.includes("pending_workspace_confirmation") || !migration.includes("ValidatedIdentityMissing")) process.exit(6);
if (!tests.includes("remembered_registry_never_implies_multi_root_authorization") || !tests.includes("registry_deduplicates_by_validated_identity_not_display_path")) process.exit(7);
console.log("ARCH-017_PR_SCOPED_VERIFY=PASS registry_not_authorization=true single_active=true validated_identity_dedup=true");
