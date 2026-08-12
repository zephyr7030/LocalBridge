import { readFileSync } from "node:fs";

const windows = readFileSync("src-tauri/src/privilege/windows.rs", "utf8");
const protocol = readFileSync("src-tauri/src/privilege/protocol.rs", "utf8");
const broker = readFileSync("src-tauri/src/privilege/broker.rs", "utf8");
const brokerMain = readFileSync("src-tauri/bin/privileged-broker/main.rs", "utf8");
const appMain = readFileSync("src-tauri/src/main.rs", "utf8");
const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
const tauri = readFileSync("src-tauri/tauri.conf.json", "utf8");

const privilegeProduction = `${windows}\n${protocol}\n${broker}\n${brokerMain}`;
for (const forbidden of ["TcpListener", "TcpStream", "UdpSocket", "0.0.0.0", "localhost:", "http://", "https://"]) {
  if (privilegeProduction.includes(forbidden)) throw new Error(`LB-011 privilege network surface detected: ${forbidden}`);
}
for (const required of ["CreateNamedPipeW", "PIPE_REJECT_REMOTE_CLIENTS", "GetNamedPipeClientProcessId", "ConvertStringSecurityDescriptorToSecurityDescriptorW", "BCryptGenRandom", "accept_elevated_client", "WaitForSingleObject"]) {
  if (!windows.includes(required)) throw new Error(`LB-011 Windows security boundary missing: ${required}`);
}
if (!windows.includes('OsStr::new("runas")') || !windows.includes("ShellExecuteExW")) throw new Error("LB-011 explicit UAC launcher missing");
for (const source of [appMain, lib]) {
  if (source.includes("launch_broker_with_explicit_uac") || source.includes('"runas"')) throw new Error("LB-011 normal/background startup auto-invokes UAC");
}
if (/requireAdministrator|highestAvailable/i.test(tauri)) throw new Error("LB-011 whole LocalBridge app requests elevation");
if (!protocol.includes("pub enum BrokerRequest { Ping, Shutdown }") || /ElevatedExec|elevated_exec|Command\s*\{|program:\s*String/.test(protocol)) throw new Error("LB-011 foundation protocol exposes an execution operation");
for (const forbidden of ["--nonce", "--secret", "--token", "--password", "--api-key"]) {
  if (broker.includes(forbidden) || brokerMain.includes(forbidden) || windows.includes(forbidden)) throw new Error(`LB-011 secret-like broker CLI flag detected: ${forbidden}`);
}
console.log("LB011_SECURITY_BOUNDARY=PASS named_pipe_only=true current_sid_acl=true expected_pid=true explicit_uac_only=true no_exec_foundation=true");
