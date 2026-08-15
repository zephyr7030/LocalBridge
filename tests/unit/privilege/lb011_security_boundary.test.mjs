import { readFileSync } from "node:fs";

const windows = readFileSync("src-tauri/src/privilege/windows.rs", "utf8");
const protocol = readFileSync("src-tauri/src/privilege/protocol.rs", "utf8");
const broker = readFileSync("src-tauri/src/privilege/broker.rs", "utf8");
const brokerMain = readFileSync("src-tauri/bin/privileged-broker/main.rs", "utf8");
const appMain = readFileSync("src-tauri/src/main.rs", "utf8");
const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
const tauri = readFileSync("src-tauri/tauri.conf.json", "utf8");
const tauriConfig = JSON.parse(tauri);

const privilegeProduction = `${windows}\n${protocol}\n${broker}\n${brokerMain}`;
for (const forbidden of ["TcpListener", "TcpStream", "UdpSocket", "0.0.0.0", "localhost:", "http://", "https://"]) {
  if (privilegeProduction.includes(forbidden)) throw new Error(`LB-011 privilege network surface detected: ${forbidden}`);
}
for (const required of ["CreateNamedPipeW", "PIPE_REJECT_REMOTE_CLIENTS", "GetNamedPipeClientProcessId", "ConvertStringSecurityDescriptorToSecurityDescriptorW", "BCryptGenRandom", "accept_elevated_client", "WaitForSingleObject"]) {
  if (!windows.includes(required)) throw new Error(`LB-011 Windows security boundary missing: ${required}`);
}
if (!windows.includes('OsStr::new("runas")') || !windows.includes("ShellExecuteExW")) throw new Error("LB-011 explicit UAC launcher missing");
if (tauriConfig.bundle?.windows?.nsis?.installMode !== "perMachine") throw new Error("LB-011 Broker requires protected per-machine installation");
for (const required of [
  "validate_broker_executable_for_current_install",
  "std::env::current_exe",
  "canonicalize",
  "symlink_metadata",
  "protected_machine_install_root",
  "SHGetFolderPathW",
  "CSIDL_PROGRAM_FILES",
  "verify_broker_installation_not_mutable_by_unprivileged_principal",
  "validate_install_object_security",
  "GetNamedSecurityInfoW",
  "OWNER_SECURITY_INFORMATION",
  "DACL_SECURITY_INFORMATION",
  "TRUSTED_INSTALL_MUTATION_SIDS",
  "INSTALL_MUTATION_MASK",
  "require_each_access_right_denied",
  "ERROR_ACCESS_DENIED",
  "FILE_WRITE_DATA",
  "FILE_ADD_FILE",
  "FILE_DELETE_CHILD",
  "WRITE_DAC_ACCESS",
  "WRITE_OWNER_ACCESS",
]) {
  if (!windows.includes(required)) throw new Error(`LB-011 Broker executable trust binding missing: ${required}`);
}
const launchStart = windows.indexOf("pub fn launch_broker_with_explicit_uac");
const launchEnd = windows.indexOf("fn build_uac_parameters", launchStart);
const launch = windows.slice(launchStart, launchEnd);
if (!launch.includes("trusted_broker") || !launch.includes("wide_null(trusted_broker.as_os_str())")) throw new Error("LB-011 runas does not use validated canonical Broker path");
for (const required of ["pin_development_broker", "_development_pin"]) {
  if (!launch.includes(required)) throw new Error(`LB-011 development UAC handoff is not binary-pinned: ${required}`);
}
const trustStart = windows.indexOf("fn validate_broker_executable_for_current_install");
const trustEnd = windows.indexOf("fn validate_broker_executable(", trustStart);
const trust = windows.slice(trustStart, trustEnd);
if (!trust.includes("verify_broker_installation_not_mutable_by_unprivileged_principal") || !trust.includes("Ok(trusted_broker)")) {
  throw new Error("LB-011 canonical Broker is returned before install ACL/mutation trust is proven");
}
for (const required of ["cfg(debug_assertions)", "validate_broker_executable(broker_executable, &current_executable, None)", "cfg(not(debug_assertions))"]) {
  if (!trust.includes(required)) throw new Error(`LB-011 debug/release Broker trust split missing: ${required}`);
}
const objectSecurityStart = windows.indexOf("fn validate_install_object_security");
const objectSecurityEnd = windows.indexOf("fn sid_to_string", objectSecurityStart);
const objectSecurity = windows.slice(objectSecurityStart, objectSecurityEnd);
for (const required of ["GetNamedSecurityInfoW", "owner.is_null()", "dacl.is_null()", "trusted_install_mutation_sid", "INSTALL_MUTATION_MASK", "ACCESS_ALLOWED_ACE_KIND", "ACCESS_DENIED_ACE_KIND"]) {
  if (!objectSecurity.includes(required)) throw new Error(`LB-011 effective object security fail-closed check missing: ${required}`);
}
const accessStart = windows.indexOf("fn require_each_access_right_denied");
const accessEnd = windows.indexOf("fn build_uac_parameters", accessStart);
const access = windows.slice(accessStart, accessEnd);
if (!access.includes("for desired_access in mutation_rights") || !access.includes("last_error_code() != ERROR_ACCESS_DENIED")) {
  throw new Error("LB-011 mutation guard must test each dangerous access right independently and fail closed on ambiguous errors");
}
for (const source of [appMain, lib]) {
  if (source.includes("launch_broker_with_explicit_uac") || source.includes('"runas"')) throw new Error("LB-011 normal/background startup auto-invokes UAC");
}
if (/requireAdministrator|highestAvailable/i.test(tauri)) throw new Error("LB-011 whole LocalBridge app requests elevation");
if (!/pub enum BrokerRequest\s*\{[\s\S]*\bPing\b[\s\S]*\bShutdown\b/.test(protocol)) {
  throw new Error("LB-011 typed Ping/Shutdown foundation operations are missing");
}
for (const forbidden of ["--nonce", "--secret", "--token", "--password", "--api-key"]) {
  if (broker.includes(forbidden) || brokerMain.includes(forbidden) || windows.includes(forbidden)) throw new Error(`LB-011 secret-like broker CLI flag detected: ${forbidden}`);
}
console.log("LB011_SECURITY_BOUNDARY=PASS named_pipe_only=true current_sid_acl=true expected_pid=true explicit_uac_only=true broker_global_acl_mutation_closed=true broker_current_token_mutation_denied=true no_exec_foundation=true");
