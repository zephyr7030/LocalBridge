# ChatGPT-side black-box client

`client.mjs` is an external MCP client. It is deliberately located outside
`src-tauri` and has no LocalBridge imports. The client only knows HTTP,
JSON-RPC, MCP protocol headers, and the `Mcp-Session-Id` returned by
`initialize`.

This placement prevents the test harness from bypassing the production
transport, policy, scheduler, ownership, or lifecycle boundaries. It also means
the client cannot discover a private runtime port from internal state. Give it
either the real loopback PEP URL produced by a test fixture or the public Tunnel
MCP URL shown by LocalBridge.

The client does not automatically reconnect, retry, poll, cancel, or translate
tool results. Those behaviors would hide the failures this suite is intended to
observe. Each output line contains the original HTTP status and JSON-RPC body.
The optional `handles` object is a non-authoritative convenience summary for an
AI to copy into the next explicit request.

Start an interactive session:

```text
npm run test:mcp-client -- --url https://example.test/mcp
```

Then send JSONL on standard input:

```json
{"op":"tools/list","request_id":"catalog"}
{"op":"tools/call","request_id":"context","name":"workspace_context","arguments":{}}
{"op":"close"}
```

One process owns exactly one MCP session. Keep the process alive across
`exec_command`, `command_control`, and `task_control` calls when testing
ownership. A tool call has the same side effects it would have from ChatGPT;
the test client itself adds no capabilities or privileged route.

The loopback PEP now requires a per-process bearer before MCP initialization and
on every request. The Rust fixture passes its test instance's bearer through
`LOCALBRIDGE_TEST_MCP_HEADERS`; production does not expose a token-discovery
endpoint. Do not put this bearer in a URL, command-line argument, or test report.
The supervised Tunnel supplies the internal bearer itself on the production path.

After `tools/list`, the client validates each actual `structuredContent` against
that tool's advertised `outputSchema` using `schema_contract.mjs`. It does not
rewrite responses, pre-validate intentional invalid inputs, or treat an absent
cached schema as an absent capability. Scenarios must discover the catalog for
each new client when checking the output contract.

`client.test.mjs` fixes the transport contract without starting LocalBridge.
`command_lifecycle.mjs` is the single black-box driver for following an accepted
public command through retryable poll-budget expiry to one durable terminal
outcome; scenarios must reuse it instead of copying polling loops.
`live_client.rs` starts the real bundled runtime and policy facade, then invokes
the external scenarios. Coverage includes queued cancellation, service shutdown
with queued side effects, session isolation and orphan adoption, phased workflow
execution ownership, DOCX package fidelity, and fail-closed multi-file Patch.
`shutdown_queue.mjs` signals the Rust fixture only after the public scheduler
reports a queued request; the fixture then invokes the real service stop path.
Neither test proves the OpenAI Tunnel path; Tunnel incidents must be reproduced
by pointing the same client at the real HTTPS connector endpoint.
