import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = process.env.LOCALBRIDGE_REPO_ROOT;
if (!root || process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-017" || process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-003") process.exit(2);
const registryPath = join(root, "src-tauri", "src", "workspace", "registry.rs");
const validatorPath = join(root, "src-tauri", "src", "workspace", "validator.rs");
const migrationPath = join(root, "src-tauri", "src", "settings", "migration.rs");
const testPath = join(root, "tests", "unit", "workspace", "workspace_registry.rs");
if (![registryPath, validatorPath, migrationPath, testPath].every(existsSync)) process.exit(3);
const registry = readFileSync(registryPath, "utf8");
const validator = readFileSync(validatorPath, "utf8");
const migration = readFileSync(migrationPath, "utf8");
const tests = readFileSync(testPath, "utf8");

function rustFiles(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return rustFiles(path);
    return path.endsWith(".rs") ? [path] : [];
  });
}

for (const required of [
  "active_workspace_id: Option<WorkspaceId>",
  "remembered_entries",
  "active_entry",
  "entry.validated_identity == incoming.validated_identity",
  "active_workspace_id = None",
  "pub struct PersistedWorkspaceIdentity",
  "validator.validate(&active.display_path)",
  "PersistedIdentityMismatch",
]) if (!registry.includes(required)) process.exit(4);

for (const required of [
  "pub struct ValidatedWorkspaceIdentity",
  "pub struct WorkspaceValidator",
  "GetFileInformationByHandle",
  "GetFinalPathNameByHandleW",
  "CreateFileW",
]) if (!validator.includes(required)) process.exit(5);
const validatedIdentityDerive = validator.match(/#\[derive\(([^)]*)\)\]\s*pub struct ValidatedWorkspaceIdentity/);
if (!validatedIdentityDerive || /Serialize|Deserialize/.test(validatedIdentityDerive[1])) process.exit(6);
if (/pub\s+fn\s+from_(?:validator|filesystem)\s*\(/.test(validator)) process.exit(7);
if (/authorized_roots|authorization_roots|display_path[^\n]{0,100}to_(?:ascii_)?lowercase/i.test(registry)) process.exit(8);
if (!migration.includes("pending_workspace_confirmation") || !migration.includes("ValidatedIdentityMissing") || !migration.includes("upsert_persisted_claim")) process.exit(9);
const productionClaimCallers = rustFiles(join(root, "src-tauri", "src"))
  .filter((path) => path !== registryPath)
  .filter((path) => /(?:from_persisted_claim|upsert_persisted_claim)\s*\(/.test(readFileSync(path, "utf8")));
if (productionClaimCallers.length !== 1 || productionClaimCallers[0] !== migrationPath) process.exit(10);
for (const requiredTest of [
  "remembered_registry_never_implies_multi_root_authorization",
  "registry_deduplicates_by_validated_identity_not_display_path",
  "deserialized_workspace_identity_is_revalidated_before_activation",
  "persisted_display_path_substitution_cannot_authorize_another_directory",
  "legitimate_persisted_workspace_is_revalidated_on_restart",
]) if (!tests.includes(requiredTest)) process.exit(11);

console.log("ARCH-017_PR_SCOPED_VERIFY=PASS registry_not_authorization=true single_active=true filesystem_revalidation=true runtime_identity_non_deserializable=true");
