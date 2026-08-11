import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = process.env.LOCALBRIDGE_REPO_ROOT;
if (!root || process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-015" || process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-005") process.exit(2);

const credentialsDir = join(root, "src-tauri", "src", "credentials");
const modulePath = join(credentialsDir, "mod.rs");
const windowsPath = join(credentialsDir, "windows.rs");
if (![modulePath, windowsPath].every(existsSync)) process.exit(3);

const moduleSource = readFileSync(modulePath, "utf8");
const windowsSource = readFileSync(windowsPath, "utf8");

for (const required of [
  "pub struct SecretString",
  "SecretString(**redacted**)",
  "pub struct CredentialMetadata",
  "credential_id",
  "credential_backend",
  "credential_backend_version",
  "has_runtime_key",
]) if (!moduleSource.includes(required)) process.exit(4);

for (const required of [
  "CredWriteW",
  "CredReadW",
  "CredDeleteW",
  "CredFree",
  "CRED_TYPE_GENERIC",
  "CRED_PERSIST_LOCAL_MACHINE",
  "classify_read_failure",
  "inaccessible_or_wrong_user_credential_fails_closed",
  "only_not_found_is_treated_as_absent",
]) if (!windowsSource.includes(required)) process.exit(5);

if (!/if\s+code\s*==\s*ERROR_NOT_FOUND_CODE[\s\S]{0,180}Ok\(None\)[\s\S]{0,260}CredentialStoreError::WindowsApi/.test(windowsSource)) process.exit(11);

const secretDerive = moduleSource.match(/#\[derive\(([^)]*)\)\]\s*pub struct SecretString/);
if (secretDerive && /Serialize|Deserialize|Clone/.test(secretDerive[1])) process.exit(6);

const credentialProduction = `${moduleSource}\n${windowsSource}`;
if (/\bstd::fs\b|\bFile::|\bOpenOptions\b|localStorage|sessionStorage|\.env\b|dotenv|RegSetValue|RegCreateKey|registry plaintext/i.test(credentialProduction)) process.exit(7);

function filesUnder(dir, predicate = () => true) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return filesUnder(path, predicate);
    return predicate(path) ? [path] : [];
  });
}

const settingsFiles = filesUnder(join(root, "src-tauri", "src", "settings"), (p) => p.endsWith(".rs"));
for (const path of settingsFiles) {
  const text = readFileSync(path, "utf8");
  if (/runtime_api_key|runtimeApiKey|api_key\s*:|credential_secret|client_secret|access_token\s*:/i.test(text)) process.exit(8);
}

const frontendFiles = filesUnder(join(root, "src"), (p) => /\.(?:ts|tsx|js|jsx)$/.test(p));
for (const path of frontendFiles) {
  const text = readFileSync(path, "utf8");
  if (/(?:localStorage|sessionStorage)[\s\S]{0,300}(?:api.?key|secret|authorization|bearer)|(?:api.?key|secret|authorization|bearer)[\s\S]{0,300}(?:localStorage|sessionStorage)/i.test(text)) process.exit(9);
}

const diagnosticsFiles = filesUnder(join(root, "src-tauri", "src", "diagnostics"), (p) => p.endsWith(".rs"));
for (const path of diagnosticsFiles) {
  const text = readFileSync(path, "utf8");
  if (/expose_secret\s*\(|read_runtime_api_key\s*\(/.test(text)) process.exit(10);
}

console.log("ARCH-015_PR_SCOPED_VERIFY=PASS windows_credential_manager=true plaintext_fallback=false frontend_secret_storage=false diagnostics_secret_read=false");
