import { createInterface } from "node:readline";
import { request as requestHttp } from "node:http";
import { request as requestHttps } from "node:https";
import { connect as connectTcp } from "node:net";
import { pathToFileURL } from "node:url";

export const MCP_PROTOCOL_VERSION = "2025-11-25";

const MAX_TIMEOUT_MS = 10 * 60 * 1000;
const DEFAULT_TIMEOUT_MS = 35_000;
const MAX_RESPONSE_BYTES = 16 * 1024 * 1024;
const RESERVED_HEADERS = new Set([
  "connection",
  "content-length",
  "host",
  "mcp-protocol-version",
  "mcp-session-id",
]);

function clientError(code, message) {
  const error = new Error(message);
  error.name = "ChatGptMcpClientError";
  error.code = code;
  return error;
}

function requestOnce(input, options, includeContentLength) {
  return new Promise((resolve, reject) => {
    const endpoint = new URL(input);
    const body = options.body == null ? null : Buffer.from(options.body);
    const headers = { ...options.headers };
    if (body != null && includeContentLength) headers["Content-Length"] = String(body.length);
    const request = (endpoint.protocol === "https:" ? requestHttps : requestHttp)(
      endpoint,
      {
        method: options.method,
        headers,
        agent: false,
      },
      (response) => {
        const chunks = [];
        let length = 0;
        response.on("data", (chunk) => {
          length += chunk.length;
          if (length > MAX_RESPONSE_BYTES) {
            request.destroy(clientError("ResponseTooLarge", "MCP response exceeds 16 MiB"));
            return;
          }
          chunks.push(chunk);
        });
        response.on("end", () => {
          const headers = {
            get(name) {
              const value = response.headers[name.toLowerCase()];
              return Array.isArray(value) ? value.join(", ") : (value ?? null);
            },
          };
          const bytes = Buffer.concat(chunks);
          resolve({
            status: response.statusCode ?? 0,
            headers,
            text: async () => bytes.toString("utf8"),
          });
        });
        response.on("aborted", () => {
          reject(clientError("ResponseAborted", "MCP response was aborted"));
        });
        response.on("error", reject);
      },
    );
    request.on("error", reject);
    if (options.signal) {
      if (options.signal.aborted) {
        request.destroy(options.signal.reason);
        return;
      }
      options.signal.addEventListener(
        "abort",
        () => request.destroy(options.signal.reason),
        { once: true },
      );
    }
    if (body != null) request.write(body);
    request.end();
  });
}

export function singleRequestFetch(input, options = {}) {
  return requestOnce(input, options, true);
}

export function emptyLoopbackPreconnect(input, timeoutMs = 5_000) {
  return new Promise((resolve, reject) => {
    const endpoint = new URL(input);
    if (endpoint.protocol !== "http:" || endpoint.hostname !== "127.0.0.1") {
      reject(clientError("InvalidEndpoint", "empty preconnect probe requires loopback HTTP"));
      return;
    }
    let settled = false;
    let receivedBytes = 0;
    const socket = connectTcp({ host: "127.0.0.1", port: Number(endpoint.port) });
    const fail = (error) => {
      if (settled) return;
      settled = true;
      reject(error);
    };
    socket.setTimeout(timeoutMs, () => {
      socket.destroy(clientError("TransportTimeout", "empty preconnect did not close"));
    });
    socket.on("connect", () => socket.end());
    socket.on("data", (chunk) => {
      receivedBytes += chunk.length;
    });
    socket.on("end", () => {
      if (settled) return;
      settled = true;
      resolve({ received_bytes: receivedBytes });
    });
    socket.on("error", fail);
  });
}

export function singleChunkedRequestFetch(input, options = {}) {
  return new Promise((resolve, reject) => {
    const endpoint = new URL(input);
    if (endpoint.protocol !== "http:" || endpoint.hostname !== "127.0.0.1") {
      reject(clientError("InvalidEndpoint", "raw chunked probe requires loopback HTTP"));
      return;
    }
    const body = options.body == null ? Buffer.alloc(0) : Buffer.from(options.body);
    const headers = { ...options.headers };
    delete headers["Content-Length"];
    headers.Host = endpoint.host;
    headers["Transfer-Encoding"] = "chunked";
    const target = `${endpoint.pathname}${endpoint.search}`;
    const lines = [`${options.method} ${target} HTTP/1.1`];
    for (const [name, value] of Object.entries(headers)) lines.push(`${name}: ${value}`);
    const head = Buffer.from(`${lines.join("\r\n")}\r\n\r\n`);
    const wire = [head];
    const first = Math.max(1, Math.floor(body.length / 3));
    const second = Math.max(first, Math.floor((body.length * 2) / 3));
    for (const chunk of [body.subarray(0, first), body.subarray(first, second), body.subarray(second)]) {
      if (chunk.length === 0) continue;
      wire.push(Buffer.from(`${chunk.length.toString(16)}\r\n`), chunk, Buffer.from("\r\n"));
    }
    wire.push(Buffer.from("0\r\n\r\n"));
    const requestBytes = Buffer.concat(wire);

    let settled = false;
    const response = [];
    let responseLength = 0;
    const socket = connectTcp({ host: "127.0.0.1", port: Number(endpoint.port) });
    const fail = (error) => {
      if (settled) return;
      settled = true;
      reject(error);
    };
    socket.setNoDelay(true);
    socket.on("connect", () => {
      socket.end(requestBytes);
    });
    socket.on("data", (chunk) => {
      responseLength += chunk.length;
      if (responseLength > MAX_RESPONSE_BYTES) {
        socket.destroy(clientError("ResponseTooLarge", "MCP response exceeds 16 MiB"));
        return;
      }
      response.push(chunk);
    });
    socket.on("end", () => {
      if (settled) return;
      const bytes = Buffer.concat(response);
      const separator = bytes.indexOf("\r\n\r\n");
      if (separator < 0) {
        fail(clientError("InvalidResponse", "MCP response has no HTTP header terminator"));
        return;
      }
      const headerLines = bytes.subarray(0, separator).toString("utf8").split("\r\n");
      const status = Number(headerLines.shift()?.split(" ")[1]);
      const responseHeaders = new Map();
      for (const line of headerLines) {
        const colon = line.indexOf(":");
        if (colon > 0) responseHeaders.set(line.slice(0, colon).toLowerCase(), line.slice(colon + 1).trim());
      }
      settled = true;
      const responseBody = bytes.subarray(separator + 4);
      resolve({
        status,
        headers: { get: (name) => responseHeaders.get(name.toLowerCase()) ?? null },
        text: async () => responseBody.toString("utf8"),
      });
    });
    socket.on("error", fail);
    if (options.signal) {
      if (options.signal.aborted) socket.destroy(options.signal.reason);
      else options.signal.addEventListener("abort", () => socket.destroy(options.signal.reason), { once: true });
    }
  });
}

export function normalizeEndpoint(value) {
  if (typeof value !== "string" || value.trim() === "") {
    throw clientError("EndpointRequired", "MCP endpoint is required");
  }
  let endpoint;
  try {
    endpoint = new URL(value);
  } catch {
    throw clientError("InvalidEndpoint", "MCP endpoint must be an absolute URL");
  }
  if (!new Set(["http:", "https:"]).has(endpoint.protocol)) {
    throw clientError("InvalidEndpoint", "MCP endpoint must use HTTP or HTTPS");
  }
  if (endpoint.username || endpoint.password || endpoint.hash) {
    throw clientError(
      "InvalidEndpoint",
      "MCP endpoint must not contain credentials or a fragment",
    );
  }
  const loopback = new Set(["127.0.0.1", "[::1]"]).has(endpoint.hostname);
  if (endpoint.protocol === "http:" && !loopback) {
    throw clientError("InsecureEndpoint", "Remote MCP endpoints must use HTTPS");
  }
  return endpoint.toString();
}

export function parseExtraHeaders(value) {
  if (value == null || value === "") return {};
  let parsed;
  try {
    parsed = JSON.parse(value);
  } catch {
    throw clientError("InvalidHeaders", "LOCALBRIDGE_TEST_MCP_HEADERS must be JSON");
  }
  if (parsed == null || Array.isArray(parsed) || typeof parsed !== "object") {
    throw clientError("InvalidHeaders", "MCP headers must be a JSON object");
  }
  const headers = {};
  for (const [name, headerValue] of Object.entries(parsed)) {
    if (RESERVED_HEADERS.has(name.toLowerCase())) {
      throw clientError("ReservedHeader", `test client owns header ${name}`);
    }
    if (typeof headerValue !== "string") {
      throw clientError("InvalidHeaders", `header ${name} must be a string`);
    }
    headers[name] = headerValue;
  }
  return headers;
}

function parseTimeout(value) {
  if (value == null) return DEFAULT_TIMEOUT_MS;
  const timeout = Number(value);
  if (!Number.isSafeInteger(timeout) || timeout < 1 || timeout > MAX_TIMEOUT_MS) {
    throw clientError(
      "InvalidTimeout",
      `timeout must be an integer between 1 and ${MAX_TIMEOUT_MS}`,
    );
  }
  return timeout;
}

function parseBody(contentType, text) {
  if (text === "") return null;
  if (!contentType.toLowerCase().includes("text/event-stream")) {
    try {
      return JSON.parse(text);
    } catch {
      throw clientError("InvalidResponse", "MCP response body is not valid JSON");
    }
  }
  const messages = text
    .split(/\r?\n/)
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice(5).trim())
    .filter((line) => line !== "" && line !== "[DONE]");
  if (messages.length !== 1) {
    throw clientError(
      "InvalidResponse",
      `expected one MCP SSE message, received ${messages.length}`,
    );
  }
  try {
    return JSON.parse(messages[0]);
  } catch {
    throw clientError("InvalidResponse", "MCP SSE data is not valid JSON");
  }
}

function collectOutputRefs(value, refs = new Set()) {
  if (Array.isArray(value)) {
    for (const item of value) collectOutputRefs(item, refs);
  } else if (value && typeof value === "object") {
    for (const [key, item] of Object.entries(value)) {
      if (key.endsWith("output_ref") && typeof item === "string") refs.add(item);
      collectOutputRefs(item, refs);
    }
  }
  return refs;
}

export function summarizeHandles(body) {
  const structured = body?.result?.structuredContent;
  const data = structured?.data;
  const error = structured?.error;
  const handles = {};
  for (const [key, value] of [
    ["task_id", data?.task_id ?? error?.task_id],
    ["execution_id", data?.execution_id],
    ["public_session_id", data?.session_id],
    ["operation_id", data?.operation_id ?? error?.operation_id],
    ["error_code", error?.code],
  ]) {
    if (typeof value === "string" && value !== "") handles[key] = value;
  }
  const refs = [...collectOutputRefs(structured ?? body)];
  if (refs.length > 0) handles.output_refs = refs;
  return handles;
}

function validateRequestId(value) {
  if (typeof value !== "string" && typeof value !== "number") {
    throw clientError("InvalidRequestId", "request_id must be a string or number");
  }
  return value;
}

export class ChatGptMcpClient {
  #endpoint;
  #extraHeaders;
  #fetch;
  #nextRequestId = 1;
  #sessionId = null;
  #timeoutMs;

  constructor({ endpoint, extraHeaders = {}, fetchImpl = singleRequestFetch, timeoutMs } = {}) {
    if (typeof fetchImpl !== "function") {
      throw clientError("FetchUnavailable", "a Fetch-compatible implementation is required");
    }
    this.#endpoint = normalizeEndpoint(endpoint);
    this.#extraHeaders = { ...extraHeaders };
    this.#fetch = fetchImpl;
    this.#timeoutMs = parseTimeout(timeoutMs);
  }

  get sessionId() {
    return this.#sessionId;
  }

  async connect() {
    if (this.#sessionId != null) {
      throw clientError("AlreadyConnected", "MCP client already owns a session");
    }
    const initialize = await this.#post({
      jsonrpc: "2.0",
      id: this.#nextRequestId++,
      method: "initialize",
      params: {
        protocolVersion: MCP_PROTOCOL_VERSION,
        capabilities: {},
        clientInfo: { name: "localbridge-chatgpt-simulator", version: "1" },
      },
    });
    if (initialize.transport.status < 200 || initialize.transport.status >= 300) {
      throw clientError(
        "InitializeFailed",
        `MCP initialize returned HTTP ${initialize.transport.status}`,
      );
    }
    const sessionId = initialize.transport.session_id;
    if (typeof sessionId !== "string" || sessionId === "") {
      throw clientError("SessionMissing", "MCP initialize did not return Mcp-Session-Id");
    }
    this.#sessionId = sessionId;
    const initialized = await this.#post({
      jsonrpc: "2.0",
      method: "notifications/initialized",
      params: {},
    });
    return { initialize, initialized };
  }

  async execute(command) {
    if (command == null || Array.isArray(command) || typeof command !== "object") {
      throw clientError("InvalidInput", "each JSONL input must be an object");
    }
    if (this.#sessionId == null) {
      throw clientError("SessionUnavailable", "MCP client is not connected");
    }
    switch (command.op) {
      case "tools/list":
        return this.#rpc("tools/list", {}, command.request_id);
      case "tools/call": {
        if (typeof command.name !== "string" || command.name === "") {
          throw clientError("InvalidInput", "tools/call requires a non-empty name");
        }
        if (
          command.arguments == null ||
          Array.isArray(command.arguments) ||
          typeof command.arguments !== "object"
        ) {
          throw clientError("InvalidInput", "tools/call arguments must be an object");
        }
        return this.#rpc(
          "tools/call",
          { name: command.name, arguments: command.arguments },
          command.request_id,
        );
      }
      case "rpc":
        if (typeof command.method !== "string" || command.method === "") {
          throw clientError("InvalidInput", "rpc requires a non-empty method");
        }
        return this.#rpc(command.method, command.params ?? {}, command.request_id);
      case "close":
        return this.close();
      default:
        throw clientError(
          "InvalidInput",
          "op must be tools/list, tools/call, rpc, or close",
        );
    }
  }

  async close() {
    if (this.#sessionId == null) {
      return { type: "closed", mcp_session_id: null, transport: { status: null } };
    }
    const ownedSession = this.#sessionId;
    const started = performance.now();
    const response = await this.#fetch(this.#endpoint, {
      method: "DELETE",
      headers: {
        ...this.#extraHeaders,
        Connection: "close",
        "MCP-Protocol-Version": MCP_PROTOCOL_VERSION,
        "Mcp-Session-Id": ownedSession,
      },
      signal: AbortSignal.timeout(this.#timeoutMs),
    });
    this.#sessionId = null;
    return {
      type: "closed",
      elapsed_ms: Math.round((performance.now() - started) * 1000) / 1000,
      mcp_session_id: ownedSession,
      transport: { status: response.status },
    };
  }

  async #rpc(method, params, suppliedRequestId) {
    const requestId = validateRequestId(suppliedRequestId ?? this.#nextRequestId++);
    const response = await this.#post({
      jsonrpc: "2.0",
      id: requestId,
      method,
      params,
    });
    return {
      type: "mcp_response",
      method,
      request_id: requestId,
      mcp_session_id: this.#sessionId,
      ...response,
      handles: summarizeHandles(response.body),
    };
  }

  async #post(payload) {
    const started = performance.now();
    const headers = {
      ...this.#extraHeaders,
      Accept: "application/json, text/event-stream",
      Connection: "close",
      "Content-Type": "application/json",
      "MCP-Protocol-Version": MCP_PROTOCOL_VERSION,
    };
    if (this.#sessionId != null) headers["Mcp-Session-Id"] = this.#sessionId;
    const response = await this.#fetch(this.#endpoint, {
      method: "POST",
      headers,
      body: JSON.stringify(payload),
      signal: AbortSignal.timeout(this.#timeoutMs),
    });
    const contentType = response.headers.get("content-type") ?? "";
    const body = parseBody(contentType, await response.text());
    return {
      elapsed_ms: Math.round((performance.now() - started) * 1000) / 1000,
      transport: {
        status: response.status,
        content_type: contentType,
        session_id: response.headers.get("mcp-session-id"),
      },
      body,
    };
  }
}

function emit(stream, value) {
  stream.write(`${JSON.stringify(value)}\n`);
}

function serializedError(error) {
  return {
    type: "client_error",
    error: {
      code: error?.code ?? error?.name ?? "ClientError",
      message: error?.message ?? String(error),
    },
  };
}

export async function runJsonLines({ client, input, output }) {
  const handshake = await client.connect();
  emit(output, {
    type: "ready",
    mcp_session_id: client.sessionId,
    protocol_version: MCP_PROTOCOL_VERSION,
    handshake,
  });
  const lines = createInterface({ input, crlfDelay: Infinity, terminal: false });
  try {
    for await (const line of lines) {
      if (line.trim() === "") continue;
      try {
        const command = JSON.parse(line);
        const result = await client.execute(command);
        emit(output, result);
        if (command.op === "close") return;
      } catch (error) {
        emit(output, serializedError(error));
      }
    }
  } finally {
    if (client.sessionId != null) {
      try {
        emit(output, await client.close());
      } catch (error) {
        emit(output, serializedError(error));
      }
    }
  }
}

export function parseArguments(args, environment = process.env) {
  const options = {
    endpoint: environment.LOCALBRIDGE_TEST_MCP_URL,
    extraHeaders: parseExtraHeaders(environment.LOCALBRIDGE_TEST_MCP_HEADERS),
    timeoutMs: environment.LOCALBRIDGE_TEST_MCP_TIMEOUT_MS,
  };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--url") options.endpoint = args[++index];
    else if (argument === "--timeout-ms") options.timeoutMs = args[++index];
    else if (argument === "--help") options.help = true;
    else throw clientError("InvalidArgument", `unknown argument: ${argument}`);
  }
  options.timeoutMs = parseTimeout(options.timeoutMs);
  return options;
}

export function usage() {
  return [
    "LocalBridge ChatGPT-side MCP simulator",
    "",
    "Usage:",
    "  node tests/black-box/chatgpt/client.mjs --url <mcp-url> [--timeout-ms <ms>]",
    "",
    "JSONL input:",
    '  {"op":"tools/list"}',
    '  {"op":"tools/call","name":"workspace_context","arguments":{}}',
    '  {"op":"tools/call","name":"command_control","arguments":{"action":"poll","session_id":"...","wait_ms":0}}',
    '  {"op":"close"}',
    "",
    "Optional environment:",
    "  LOCALBRIDGE_TEST_MCP_URL",
    "  LOCALBRIDGE_TEST_MCP_TIMEOUT_MS",
    "  LOCALBRIDGE_TEST_MCP_HEADERS (JSON object; values are never printed)",
  ].join("\n");
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(`${usage()}\n`);
    return;
  }
  const client = new ChatGptMcpClient(options);
  await runJsonLines({ client, input: process.stdin, output: process.stdout });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`${JSON.stringify(serializedError(error))}\n`);
    process.exitCode = 1;
  });
}
