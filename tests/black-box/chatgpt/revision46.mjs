import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";

import {
  ChatGptMcpClient,
  emptyLoopbackPreconnect,
  singleChunkedRequestFetch,
} from "./client.mjs";
import {
  drivePublicCommandToTerminal,
  settleAcceptedPublicCommand,
} from "./command_lifecycle.mjs";

function argumentsFrom(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--url") options.endpoint = argv[++index];
    else if (argument === "--workspace") options.workspace = argv[++index];
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (!options.endpoint || !options.workspace) {
    throw new Error("--url and --workspace are required");
  }
  return options;
}

function structured(response) {
  return response.body?.result?.structuredContent;
}

function explain(response) {
  return JSON.stringify(response, null, 2);
}

function assertSuccess(response) {
  assert.equal(response.transport.status, 200, explain(response));
  assert.equal(response.body?.result?.isError, false, explain(response));
  assert.equal(structured(response)?.ok, true, explain(response));
  assert.equal(structured(response)?.error, null, explain(response));
  return structured(response).data;
}

function assertToolError(response, code) {
  assert.equal(response.transport.status, 200, explain(response));
  assert.equal(response.body?.result?.isError, true, explain(response));
  assert.equal(structured(response)?.ok, false, explain(response));
  assert.equal(structured(response)?.error?.code, code, explain(response));
  return structured(response).error;
}

function toolSchema(tools, name) {
  const tool = tools.find((candidate) => candidate.name === name);
  assert.ok(tool, `missing public tool ${name}`);
  return tool.inputSchema;
}

async function toolCall(client, name, args, requestId) {
  try {
    return await client.execute({
      op: "tools/call",
      request_id: requestId,
      name,
      arguments: args,
    });
  } catch (error) {
    const cause = error?.cause ? `; cause=${error.cause.code ?? error.cause}` : "";
    throw new Error(`${requestId} transport failed: ${error.message}${cause}`, { cause: error });
  }
}

export async function runRevision46Scenario({ endpoint, workspace }) {
  const client = new ChatGptMcpClient({ endpoint, timeoutMs: 35_000 });
  const reconnectClient = new ChatGptMcpClient({ endpoint, timeoutMs: 35_000 });
  const report = {
    mcp_session_id: null,
    checks: {},
    tunnel: "NOT_RUN_LOCAL_PEP_ONLY",
  };
  try {
    await client.connect();
  } catch (error) {
    throw new Error(`initialize transport failed: ${error.message}`, { cause: error });
  }
  report.mcp_session_id = client.sessionId;
  try {
    let listed;
    try {
      listed = await client.execute({ op: "tools/list", request_id: "catalog" });
    } catch (error) {
      throw new Error(`catalog transport failed: ${error.message}`, { cause: error });
    }
    assert.equal(listed.transport.status, 200, explain(listed));
    const tools = listed.body.result.tools;
    assert.deepEqual(toolSchema(tools, "command_control").properties.action.enum, [
      "adopt",
      "poll",
      "read",
      "write",
      "kill",
    ]);
    assert.deepEqual(toolSchema(tools, "task_control").properties.action.enum, [
      "list",
      "get",
      "cancel",
    ]);
    assert.ok(toolSchema(tools, "task_control").properties.task_id);
    assert.ok(toolSchema(tools, "agent_workflow").properties.task_id);
    const documentSchema = toolSchema(tools, "document_workflow");
    assert.deepEqual(documentSchema.properties.action.enum, [
      "inspect",
      "search",
      "create",
      "edit",
      "convert",
      "rebuild",
    ]);
    assert.deepEqual(documentSchema.properties.edits.items.properties.operation.enum, [
      "replace",
      "insert_before",
      "insert_after",
      "delete",
    ]);
    assert.equal(documentSchema.properties.expected_sha256.minLength, 64);
    await reconnectClient.connect();
    report.checks.public_schema = "PASS";

    const listedFiles = await toolCall(
      client,
      "filesystem",
      { action: "list", path: ".", max_entries: 20, sort_by: "path" },
      "filesystem-list",
    );
    const listData = assertSuccess(listedFiles);
    assert.ok(listData.entries.length >= 2, explain(listedFiles));
    const searched = await toolCall(
      client,
      "filesystem",
      {
        action: "search",
        path: ".",
        pattern: "range.txt",
        recursive: true,
        max_depth: 2,
        max_results: 20,
      },
      "filesystem-search",
    );
    const searchData = assertSuccess(searched);
    assert.ok(
      searchData.entries.some((entry) => entry.path.replaceAll("\\", "/").endsWith("range.txt")),
      explain(searched),
    );
    report.checks.filesystem_enumeration = "PASS";

    const absoluteTarget = `${workspace}\\absolute-equivalence.txt`;
    const absoluteWrite = await toolCall(
      client,
      "exec_command",
      {
        command: `echo LB_ABSOLUTE_EQUIVALENCE>"${absoluteTarget}"`,
        shell: "cmd",
        yield_time_ms: 10_000,
      },
      "absolute-redirection",
    );
    const absoluteTerminal = await settleAcceptedPublicCommand({
      initialResponse: absoluteWrite,
      callTool: (name, args, requestId) => toolCall(client, name, args, requestId),
      requestPrefix: "absolute-redirection-poll",
    });
    assert.equal(assertSuccess(absoluteTerminal).status, "completed", explain(absoluteTerminal));
    const relativeWrite = await toolCall(
      client,
      "exec_command",
      {
        command: "echo LB_RELATIVE_EQUIVALENCE>relative-equivalence.txt",
        shell: "cmd",
        workdir: ".",
        yield_time_ms: 10_000,
      },
      "relative-redirection",
    );
    const relativeTerminal = await settleAcceptedPublicCommand({
      initialResponse: relativeWrite,
      callTool: (name, args, requestId) => toolCall(client, name, args, requestId),
      requestPrefix: "relative-redirection-poll",
    });
    assert.equal(assertSuccess(relativeTerminal).status, "completed", explain(relativeTerminal));
    for (const [requestId, path] of [
      ["absolute-stat", "absolute-equivalence.txt"],
      ["relative-stat", "relative-equivalence.txt"],
      ["policy-probe-stat", "policy_probe.cmd"],
    ]) {
      const stat = await toolCall(
        client,
        "filesystem",
        { action: "stat", path },
        requestId,
      );
      assert.equal(assertSuccess(stat).kind, "file", explain(stat));
    }
    report.checks.workspace_path_equivalence = "PASS";

    const authorityContext = await toolCall(
      client,
      "workspace_context",
      {},
      "ordinary-authority-context",
    );
    assert.equal(
      assertSuccess(authorityContext).ordinary_route_token,
      "current_windows_user",
      explain(authorityContext),
    );
    const directCurrentUser = await toolCall(
      client,
      "exec_command",
      { command: "sc query EventLog", shell: "cmd", yield_time_ms: 10_000 },
      "direct-current-user",
    );
    const directCurrentUserTerminal = await settleAcceptedPublicCommand({
      initialResponse: directCurrentUser,
      callTool: (name, args, requestId) => toolCall(client, name, args, requestId),
      requestPrefix: "direct-current-user-poll",
    });
    const directCurrentUserData = assertSuccess(directCurrentUserTerminal);
    assert.equal(
      directCurrentUserData.status,
      "completed",
      explain(directCurrentUserTerminal),
    );
    assert.match(
      directCurrentUserData.output,
      /SERVICE_NAME:\s*EventLog/i,
      explain(directCurrentUser),
    );
    const descendantCurrentUser = await toolCall(
      client,
      "exec_command",
      {
        command: "call .\\policy_probe.cmd",
        shell: "cmd",
        workdir: ".",
        yield_time_ms: 10_000,
      },
      "descendant-current-user",
    );
    const descendantCurrentUserTerminal = await settleAcceptedPublicCommand({
      initialResponse: descendantCurrentUser,
      callTool: (name, args, requestId) => toolCall(client, name, args, requestId),
      requestPrefix: "descendant-current-user-poll",
    });
    const descendantCurrentUserData = assertSuccess(descendantCurrentUserTerminal);
    assert.equal(
      descendantCurrentUserData.status,
      "completed",
      explain(descendantCurrentUserTerminal),
    );
    assert.match(
      descendantCurrentUserData.output,
      /SERVICE_NAME:\s*EventLog/i,
      explain(descendantCurrentUserTerminal),
    );
    report.checks.descendant_process_authority = "PASS_CURRENT_USER_PARITY";

    const interactive = await toolCall(
      client,
      "exec_command",
      {
        command:
          "$line=[Console]::In.ReadLine(); Write-Output ('LB_WRITE_GOT='+$line); Start-Sleep -Seconds 20",
        shell: "windows_powershell",
        yield_time_ms: 0,
        timeout_ms: 60_000,
      },
      "interactive-start",
    );
    const interactiveData = assertSuccess(interactive);
    assert.equal(interactiveData.status, "running", explain(interactive));
    const interactiveSession = interactiveData.session_id;
    const written = await toolCall(
      client,
      "command_control",
      {
        action: "write",
        session_id: interactiveSession,
        chars: "black-box-input\n",
        wait_ms: 0,
      },
      "interactive-write",
    );
    assert.ok(written.elapsed_ms < 1_500, explain(written));
    assert.notEqual(structured(written)?.error?.code, "SessionUnavailable", explain(written));
    const killed = await toolCall(
      client,
      "command_control",
      { action: "kill", session_id: interactiveSession, signal: "KILL", wait_ms: 0 },
      "interactive-kill",
    );
    assert.ok(killed.elapsed_ms < 1_500, explain(killed));
    assert.notEqual(structured(killed)?.error?.code, "SessionUnavailable", explain(killed));
    const killedTerminal = await drivePublicCommandToTerminal({
      callTool: (name, args, requestId) => toolCall(client, name, args, requestId),
      publicSessionId: interactiveSession,
      requestPrefix: "killed-poll",
    });
    const killedData = assertSuccess(killedTerminal);
    assert.equal(killedData.status, "cancelled", explain(killedTerminal));
    report.checks.command_session_ownership = "PASS";
    report.checks.command_wait_budget = {
      status: "PASS",
      write_elapsed_ms: written.elapsed_ms,
      kill_elapsed_ms: killed.elapsed_ms,
    };
    report.checks.cancelled_envelope = "PASS";

    const cancellable = await toolCall(
      client,
      "exec_command",
      {
        command: "Start-Sleep -Seconds 30; Write-Output SHOULD_NOT_COMPLETE",
        shell: "windows_powershell",
        yield_time_ms: 0,
        timeout_ms: 60_000,
      },
      "task-cancel-start",
    );
    const cancellableData = assertSuccess(cancellable);
    assert.equal(cancellableData.status, "running", explain(cancellable));
    const observedCancellable = await toolCall(
      client,
      "task_control",
      { action: "get", task_id: cancellableData.task_id },
      "task-cancel-observe",
    );
    assert.equal(
      assertSuccess(observedCancellable).task.id,
      cancellableData.task_id,
      explain(observedCancellable),
    );
    const isolatedCancel = await toolCall(
      reconnectClient,
      "task_control",
      { action: "cancel" },
      "task-cancel-reconnect-without-id",
    );
    const isolatedError = assertToolError(isolatedCancel, "TaskIdRequired");
    assert.equal(isolatedError.details.task_id, cancellableData.task_id, explain(isolatedCancel));
    const taskCancel = await toolCall(
      client,
      "task_control",
      { action: "cancel", task_id: cancellableData.task_id },
      "task-cancel",
    );
    const taskCancelData = assertSuccess(taskCancel);
    assert.ok(taskCancel.elapsed_ms < 2_000, explain(taskCancel));
    assert.ok(taskCancelData.cancelled_requests >= 1, explain(taskCancel));
    const taskTerminal = await drivePublicCommandToTerminal({
      callTool: (name, args, requestId) => toolCall(client, name, args, requestId),
      publicSessionId: cancellableData.session_id,
      requestPrefix: "task-cancel-poll",
    });
    assert.equal(assertSuccess(taskTerminal).status, "cancelled", explain(taskTerminal));
    report.checks.task_cancel_detached = "PASS";

    const prepared = await toolCall(
      client,
      "agent_workflow",
      {
        action: "diagnose",
        phase: "prepare",
        objective: "black-box prepared workflow cancellation",
      },
      "workflow-prepare",
    );
    const preparedData = assertSuccess(prepared);
    assert.equal(preparedData.state, "prepared", explain(prepared));
    const resumed = await toolCall(
      reconnectClient,
      "agent_workflow",
      { action: "resume", task_id: preparedData.task_id },
      "workflow-resume-after-reconnect",
    );
    const resumedData = assertSuccess(resumed);
    assert.equal(resumedData.task_id, preparedData.task_id, explain(resumed));
    assert.equal(resumedData.state, "prepared", explain(resumed));
    const workflowCancel = await toolCall(
      reconnectClient,
      "task_control",
      { action: "cancel", task_id: preparedData.task_id },
      "workflow-cancel",
    );
    const workflowCancelData = assertSuccess(workflowCancel);
    assert.equal(workflowCancelData.workflow_cancelled, true, explain(workflowCancel));
    assert.equal(workflowCancelData.durable_task_cancelled, true, explain(workflowCancel));
    const replacement = await toolCall(
      reconnectClient,
      "agent_workflow",
      {
        action: "diagnose",
        phase: "prepare",
        objective: "replacement after cancelled workflow",
      },
      "workflow-replacement",
    );
    const replacementData = assertSuccess(replacement);
    assert.equal(replacementData.state, "prepared", explain(replacement));
    assertSuccess(
      await toolCall(
        reconnectClient,
        "task_control",
        { action: "cancel", task_id: replacementData.task_id },
        "workflow-replacement-cancel",
      ),
    );
    report.checks.prepared_workflow_cancel = "PASS";
    report.checks.cross_session_workflow_resume = "PASS";

    for (const [requestId, action, field] of [
      ["git-show-invalid", "show", "rev"],
      ["git-log-invalid", "log", "ref"],
    ]) {
      const response = await toolCall(
        client,
        "git_workflow",
        { action, [field]: "definitely-not-a-ref" },
        requestId,
      );
      assert.equal(response.body.result.isError, true, explain(response));
      assert.equal(structured(response).ok, false, explain(response));
      assert.match(
        structured(response).error.details.git_message,
        /definitely-not-a-ref/i,
        explain(response),
      );
    }
    const blamePastEof = await toolCall(
      client,
      "git_workflow",
      {
        action: "blame",
        path: "range.txt",
        start_line: 999,
        end_line: 1_000,
      },
      "git-blame-past-eof",
    );
    assert.equal(blamePastEof.body.result.isError, true, explain(blamePastEof));
    assert.equal(structured(blamePastEof).ok, false, explain(blamePastEof));
    report.checks.git_error_propagation = "PASS";

    const documentInspect = await toolCall(
      client,
      "document_workflow",
      { action: "inspect", path: "range.txt", start_block: 1, max_blocks: 2 },
      "document-inspect",
    );
    const documentData = assertSuccess(documentInspect);
    assert.equal(documentData.blocks.length, 2, explain(documentInspect));
    assert.equal(documentData.blocks[1].id, "block-2", explain(documentInspect));
    assert.equal(documentData.truncated, true, explain(documentInspect));
    assert.match(documentData.sha256, /^[a-f0-9]{64}$/i, explain(documentInspect));

    const documentSearch = await toolCall(
      client,
      "document_workflow",
      { action: "search", path: "range.txt", query: "line3" },
      "document-search",
    );
    const documentSearchData = assertSuccess(documentSearch);
    assert.equal(documentSearchData.matches[0].block_id, "block-3", explain(documentSearch));

    const documentEdit = await toolCall(
      client,
      "document_workflow",
      {
        action: "edit",
        path: "range.txt",
        expected_sha256: documentData.sha256,
        edits: [{ operation: "replace", block_id: "block-2", content: "line2-edited" }],
      },
      "document-edit",
    );
    const editData = assertSuccess(documentEdit);
    assert.notEqual(editData.sha256, documentData.sha256, explain(documentEdit));
    const staleEdit = await toolCall(
      client,
      "document_workflow",
      {
        action: "edit",
        path: "range.txt",
        expected_sha256: documentData.sha256,
        edits: [{ operation: "delete", block_id: "block-1" }],
      },
      "document-stale-edit",
    );
    assertToolError(staleEdit, "FileChanged");

    const createDocx = await toolCall(
      client,
      "document_workflow",
      {
        action: "create",
        path: "document-probe.docx",
        source_format: "markdown",
        content: "# Document Probe\nDOCX_SEARCH_NEEDLE",
      },
      "document-create-docx",
    );
    assertSuccess(createDocx);
    const searchDocx = await toolCall(
      client,
      "document_workflow",
      { action: "search", path: "document-probe.docx", query: "DOCX_SEARCH_NEEDLE" },
      "document-search-docx",
    );
    assert.equal(assertSuccess(searchDocx).matches.length, 1, explain(searchDocx));
    const convertDocx = await toolCall(
      client,
      "document_workflow",
      { action: "convert", source: "document-probe.docx", path: "document-probe.md" },
      "document-convert-docx",
    );
    assertSuccess(convertDocx);
    const inspectConverted = await toolCall(
      client,
      "document_workflow",
      { action: "inspect", path: "document-probe.md" },
      "document-inspect-converted",
    );
    const convertedData = assertSuccess(inspectConverted);
    assert.equal(convertedData.blocks[0].kind, "heading", explain(inspectConverted));
    assert.equal(convertedData.blocks[0].text, "Document Probe", explain(inspectConverted));
    const searchPdf = await toolCall(
      client,
      "document_workflow",
      { action: "search", path: "document-probe.pdf", query: "PDF_SEARCH_NEEDLE" },
      "document-search-pdf",
    );
    const pdfData = assertSuccess(searchPdf);
    assert.equal(pdfData.format, "pdf", explain(searchPdf));
    assert.equal(pdfData.matches.length, 1, explain(searchPdf));
    const convertPdf = await toolCall(
      client,
      "document_workflow",
      { action: "convert", source: "document-probe.pdf", path: "document-probe.txt" },
      "document-convert-pdf",
    );
    assertSuccess(convertPdf);
    const editPdf = await toolCall(
      client,
      "document_workflow",
      {
        action: "edit",
        path: "document-probe.pdf",
        expected_sha256: pdfData.sha256,
        edits: [{ operation: "delete", block_id: "block-1" }],
      },
      "document-edit-pdf",
    );
    assertToolError(editPdf, "InvalidArgument");
    report.checks.document_workflow = "PASS";

    const missingOutput = await toolCall(
      client,
      "command_control",
      { action: "read", output_ref: "lb-output-invalid", stream: "stdout" },
      "output-missing",
    );
    assertToolError(missingOutput, "OutputNotFound");
    const streamProbe = await toolCall(
      client,
      "exec_command",
      {
        command: "Write-Output LB_STREAM_PROBE",
        shell: "windows_powershell",
        yield_time_ms: 10_000,
      },
      "stream-probe",
    );
    const streamTerminal = await settleAcceptedPublicCommand({
      initialResponse: streamProbe,
      callTool: (name, args, requestId) => toolCall(client, name, args, requestId),
      requestPrefix: "stream-probe-poll",
    });
    const streamData = assertSuccess(streamTerminal);
    assert.equal(streamData.status, "completed", explain(streamTerminal));
    const stdoutRef = streamData.output_refs.stdout;
    const mismatchedStream = await toolCall(
      client,
      "command_control",
      { action: "read", output_ref: stdoutRef, stream: "stderr" },
      "output-stream-mismatch",
    );
    const mismatchError = assertToolError(mismatchedStream, "InvalidArgument");
    assert.equal(mismatchError.details.field, "stream", explain(mismatchedStream));
    assert.equal(mismatchError.details.expected, "stdout", explain(mismatchedStream));
    assert.equal(mismatchError.details.actual, "stderr", explain(mismatchedStream));
    report.checks.output_error_taxonomy = "PASS";

    const finalTask = await toolCall(
      client,
      "task_control",
      { action: "get" },
      "final-task-state",
    );
    const finalTaskData = assertSuccess(finalTask);
    assert.equal(finalTaskData.scheduler.detached_executions_running, 0, explain(finalTask));
    assert.equal(finalTaskData.scheduler.foreground_work_running, 0, explain(finalTask));
    assert.equal(finalTaskData.scheduler.queue_depth, 0, explain(finalTask));
    assert.equal(finalTaskData.current_activity, null, explain(finalTask));
    report.checks.final_projection = "PASS";
    return report;
  } finally {
    try {
      await reconnectClient.close();
    } catch (error) {
      throw new Error(`reconnected session close failed: ${error.message}`, { cause: error });
    }
    try {
      await client.close();
    } catch (error) {
      throw new Error(`session close transport failed: ${error.message}`, { cause: error });
    }
  }
}

export async function runChunkedTransportScenario(endpoint) {
  const client = new ChatGptMcpClient({
    endpoint,
    timeoutMs: 10_000,
    fetchImpl: singleChunkedRequestFetch,
  });
  try {
    await client.connect();
  } catch (error) {
    throw new Error(`chunked initialize failed: ${error.message}`, { cause: error });
  }
  try {
    for (let index = 0; index < 50; index += 1) {
      const empty = await emptyLoopbackPreconnect(endpoint);
      assert.equal(empty.received_bytes, 0, explain(empty));
      let response;
      try {
        response = await client.execute({
          op: "tools/list",
          request_id: `chunked-${index}`,
        });
      } catch (error) {
        throw new Error(`chunked-${index} failed: ${error.message}`, { cause: error });
      }
      assert.equal(response.transport.status, 200, explain(response));
      assert.ok(response.body.result.tools.length > 0, explain(response));
    }
    return "PASS_50_CHUNKED_50_EMPTY_PRECONNECTS";
  } finally {
    try {
      await client.close();
    } catch (error) {
      throw new Error(`chunked close failed: ${error.message}`, { cause: error });
    }
  }
}

async function main() {
  const options = argumentsFrom(process.argv.slice(2));
  const report = await runRevision46Scenario(options);
  report.chunked_local_transport = await runChunkedTransportScenario(options.endpoint);
  process.stdout.write(`${JSON.stringify(report)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error}\n`);
    if (error.cause) process.stderr.write(`cause: ${error.cause.stack ?? error.cause}\n`);
    process.exitCode = 1;
  });
}
