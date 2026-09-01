import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

import {
  ChatGptMcpClient,
  MCP_PROTOCOL_VERSION,
  normalizeEndpoint,
  parseExtraHeaders,
} from "./client.mjs";

async function startMcpFixture() {
  const requests = [];
  const server = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const text = Buffer.concat(chunks).toString("utf8");
    const body = text === "" ? null : JSON.parse(text);
    requests.push({
      method: request.method,
      protocol: request.headers["mcp-protocol-version"],
      session: request.headers["mcp-session-id"],
      connection: request.headers.connection,
      body,
    });

    if (request.method === "DELETE") {
      response.writeHead(204);
      response.end();
      return;
    }
    if (body?.method === "initialize") {
      response.writeHead(200, {
        "content-type": "application/json",
        "mcp-session-id": "mcp-session-test-a",
      });
      response.end(
        JSON.stringify({
          jsonrpc: "2.0",
          id: body.id,
          result: {
            protocolVersion: MCP_PROTOCOL_VERSION,
            capabilities: { tools: { listChanged: true } },
            serverInfo: { name: "fixture", version: "1" },
          },
        }),
      );
      return;
    }
    if (body?.method === "notifications/initialized") {
      response.writeHead(202);
      response.end();
      return;
    }
    if (body?.method === "tools/list") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(
        JSON.stringify({
          jsonrpc: "2.0",
          id: body.id,
          result: { tools: [{ name: "exec_command" }] },
        }),
      );
      return;
    }
    if (body?.method === "tools/call") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(
        JSON.stringify({
          jsonrpc: "2.0",
          id: body.id,
          result: {
            isError: false,
            structuredContent: {
              data: {
                status: "running",
                task_id: "lb-task-test",
                execution_id: "lb-execution-test",
                session_id: "lb-session-test",
                stdout: { output_ref: "lb-output-test" },
              },
            },
          },
        }),
      );
      return;
    }
    response.writeHead(400, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: "unexpected request" }));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  return {
    endpoint: `http://127.0.0.1:${address.port}/mcp`,
    requests,
    close: () =>
      new Promise((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      ),
  };
}

test("simulator uses one real MCP session for list, call, and close", async () => {
  const fixture = await startMcpFixture();
  try {
    const client = new ChatGptMcpClient({ endpoint: fixture.endpoint, timeoutMs: 2_000 });
    const handshake = await client.connect();
    assert.equal(client.sessionId, "mcp-session-test-a");
    assert.equal(handshake.initialize.transport.status, 200);
    assert.equal(handshake.initialized.transport.status, 202);

    const listed = await client.execute({ op: "tools/list", request_id: "list-a" });
    assert.equal(listed.transport.status, 200);
    assert.equal(listed.body.result.tools[0].name, "exec_command");

    const called = await client.execute({
      op: "tools/call",
      request_id: 7,
      name: "exec_command",
      arguments: { command: "probe" },
    });
    assert.deepEqual(called.handles, {
      task_id: "lb-task-test",
      execution_id: "lb-execution-test",
      public_session_id: "lb-session-test",
      output_refs: ["lb-output-test"],
    });

    const closed = await client.close();
    assert.equal(closed.transport.status, 204);
    assert.equal(client.sessionId, null);

    assert.equal(fixture.requests.length, 5);
    assert.equal(fixture.requests[0].body.method, "initialize");
    assert.equal(fixture.requests[0].session, undefined);
    for (const request of fixture.requests) {
      assert.equal(request.protocol, MCP_PROTOCOL_VERSION);
      assert.equal(request.connection, "close");
    }
    for (const request of fixture.requests.slice(1)) {
      assert.equal(request.session, "mcp-session-test-a");
    }
    assert.deepEqual(
      fixture.requests.slice(1, 4).map((request) => request.body.method),
      ["notifications/initialized", "tools/list", "tools/call"],
    );
  } finally {
    await fixture.close();
  }
});

test("simulator refuses insecure remote endpoints and reserved transport headers", () => {
  assert.throws(() => normalizeEndpoint("http://example.test/mcp"), {
    code: "InsecureEndpoint",
  });
  assert.equal(normalizeEndpoint("https://example.test/mcp"), "https://example.test/mcp");
  assert.throws(() => parseExtraHeaders('{"Mcp-Session-Id":"forged"}'), {
    code: "ReservedHeader",
  });
});
