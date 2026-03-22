import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

import { createTestEnvironment, projectRoot, runProcess } from "./lib/test-env.mjs";

class McpLauncherClient {
  constructor(child, rootUri) {
    this.child = child;
    this.rootUri = rootUri;
    this.buffer = Buffer.alloc(0);
    this.pending = new Map();
    this.nextId = 1;
    this.exitResult = null;
    this.exitPromise = new Promise((resolve) => {
      child.once("exit", (code, signal) => {
        this.exitResult = { code, signal };
        for (const pending of this.pending.values()) {
          pending.reject(new Error(`launcher exited early with code ${code}`));
        }
        this.pending.clear();
        resolve(this.exitResult);
      });
    });

    child.stdout.on("data", (chunk) => {
      this.buffer = Buffer.concat([this.buffer, chunk]);
      while (true) {
        const headerEnd = this.buffer.indexOf("\r\n\r\n");
        if (headerEnd === -1) {
          return;
        }

        const header = this.buffer.subarray(0, headerEnd).toString("utf8");
        const contentLengthLine = header
          .split(/\r?\n/u)
          .find((line) => line.toLowerCase().startsWith("content-length:"));
        if (!contentLengthLine) {
          throw new Error(`Missing Content-Length header: ${header}`);
        }

        const length = Number(contentLengthLine.split(":")[1].trim());
        const bodyStart = headerEnd + 4;
        if (this.buffer.length < bodyStart + length) {
          return;
        }

        const payload = this.buffer.subarray(bodyStart, bodyStart + length);
        this.buffer = this.buffer.subarray(bodyStart + length);
        this.handleMessage(JSON.parse(payload.toString("utf8")));
      }
    });
  }

  send(message) {
    const payload = Buffer.from(JSON.stringify(message), "utf8");
    this.child.stdin.write(`content-length: ${payload.length}\r\n\r\n`);
    this.child.stdin.write(payload);
  }

  request(method, params = {}) {
    const id = String(this.nextId++);
    this.send({
      jsonrpc: "2.0",
      id,
      method,
      params
    });

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Timed out waiting for ${method}`));
      }, 5000);

      this.pending.set(id, {
        resolve: (result) => {
          clearTimeout(timer);
          resolve(result);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        }
      });
    });
  }

  notify(method, params = {}) {
    this.send({
      jsonrpc: "2.0",
      method,
      params
    });
  }

  handleMessage(message) {
    if (message.method === "roots/list") {
      this.send({
        jsonrpc: "2.0",
        id: message.id,
        result: {
          roots: [{ uri: this.rootUri, name: "integration" }]
        }
      });
      return;
    }

    if (message.method) {
      this.send({
        jsonrpc: "2.0",
        id: message.id,
        error: {
          code: -32601,
          message: `Unhandled server request ${message.method}`
        }
      });
      return;
    }

    const pending = this.pending.get(String(message.id));
    if (!pending) {
      return;
    }

    this.pending.delete(String(message.id));
    if (message.error) {
      pending.reject(new Error(message.error.message));
      return;
    }

    pending.resolve(message.result);
  }

  async initialize() {
    const result = await this.request("initialize", {
      protocolVersion: "2025-11-25",
      capabilities: {
        roots: {
          listChanged: true
        }
      },
      clientInfo: {
        name: "spiki-program-exec-test",
        version: "0.1.0"
      }
    });
    this.notify("notifications/initialized");
    return result;
  }

  async close() {
    if (!this.child.stdin.destroyed && !this.child.stdin.writableEnded) {
      this.child.stdin.end();
    }

    const result = await this.exitPromise;
    if (result.code !== 0) {
      throw new Error(`launcher exited with code ${result.code}`);
    }
  }
}

test("spiki launcher handles pipelined initialize and first tool call", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-pipeline-",
    files: {
      "index.ts": "const answer = 42;\n"
    }
  });
  t.after(async () => {
    await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "stop"], {
      cwd: projectRoot,
      env: context.env,
      timeoutMs: 5000
    }).catch(() => {});
    await context.cleanup();
  });

  const child = spawn(process.execPath, ["./bin/spiki.js"], {
    cwd: projectRoot,
    env: context.env,
    stdio: ["pipe", "pipe", "inherit"]
  });
  const client = new McpLauncherClient(child, context.rootUri);
  t.after(async () => {
    await client.close().catch(() => {});
  });

  const initializePromise = client.request("initialize", {
    protocolVersion: "2025-11-25",
    capabilities: {
      roots: {
        listChanged: true
      }
    },
    clientInfo: {
      name: "spiki-pipeline-test",
      version: "0.1.0"
    }
  });
  const workspaceStatusPromise = client.request("tools/call", {
    name: "ae.workspace.status",
    arguments: {
      includeCoverage: true,
      includeBackends: true
    }
  });

  const initialize = await initializePromise;
  const workspaceStatus = await workspaceStatusPromise;
  client.notify("notifications/initialized");

  assert.equal(initialize.serverInfo.name, "spiki");
  assert.equal(workspaceStatus.isError, false);
  assert.equal(workspaceStatus.structuredContent.workspaceRevision, "rev_1");
});

test("spiki CLI and launcher bridge manage daemon lifecycle", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-program-",
    files: {
      "index.ts": "const needle = 1;\nconsole.log(needle);\n",
      "nested/example.ts": "export const nestedValue = needle;\n"
    }
  });
  t.after(async () => {
    await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "stop"], {
      cwd: projectRoot,
      env: context.env,
      timeoutMs: 5000
    }).catch(() => {});
    await context.cleanup();
  });

  const initialStatus = await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "status"], {
    cwd: projectRoot,
    env: context.env
  });
  assert.equal(initialStatus.code, 0, initialStatus.stderr);
  const initialStatusJson = JSON.parse(initialStatus.stdout);
  assert.equal(initialStatusJson.runtimeDir, context.runtimeDir);
  assert.equal(initialStatusJson.reachable, false);

  const doctorBefore = await runProcess(process.execPath, ["./bin/spiki.js", "doctor"], {
    cwd: projectRoot,
    env: context.env
  });
  assert.equal(doctorBefore.code, 0, doctorBefore.stderr);
  assert.ok(doctorBefore.stdout.includes(`runtimeDir: ${context.runtimeDir}`));
  assert.match(doctorBefore.stdout, /reachable: false/);

  const child = spawn(process.execPath, ["./bin/spiki.js"], {
    cwd: projectRoot,
    env: context.env,
    stdio: ["pipe", "pipe", "inherit"]
  });
  const client = new McpLauncherClient(child, context.rootUri);
  t.after(async () => {
    await client.close().catch(() => {});
  });

  const initialize = await client.initialize();
  assert.equal(initialize.serverInfo.name, "spiki");

  const tools = await client.request("tools/list");
  const toolNames = tools.tools.map((tool) => tool.name).sort();
  assert.deepEqual(toolNames, [
    "ae.edit.apply_plan",
    "ae.edit.discard_plan",
    "ae.edit.prepare_plan",
    "ae.semantic.ensure",
    "ae.semantic.status",
    "ae.workspace.read_spans",
    "ae.workspace.search_text",
    "ae.workspace.status"
  ]);

  const workspaceStatus = await client.request("tools/call", {
    name: "ae.workspace.status",
    arguments: {
      includeCoverage: true,
      includeBackends: true
    }
  });
  assert.equal(workspaceStatus.isError, false);
  assert.equal(workspaceStatus.structuredContent.roots[0], context.rootUri);
  assert.equal(workspaceStatus.structuredContent.workspaceRevision, "rev_1");

  const readSpans = await client.request("tools/call", {
    name: "ae.workspace.read_spans",
    arguments: {
      spans: [
        {
          uri: pathToFileURL(path.join(context.workspaceDir, "index.ts")).toString(),
          range: {
            start: { line: 1, character: 0 },
            end: { line: 1, character: 12 }
          },
          contextLines: 1
        }
      ]
    }
  });
  assert.equal(readSpans.isError, false);
  assert.match(readSpans.structuredContent.spans[0].text, /console\.log/);

  const search = await client.request("tools/call", {
    name: "ae.workspace.search_text",
    arguments: {
      query: "needle",
      mode: "literal",
      limit: 10
    }
  });
  assert.equal(search.isError, false);
  assert.equal(search.structuredContent.matches.length, 3);

  const semanticStatus = await client.request("tools/call", {
    name: "ae.semantic.status",
    arguments: {}
  });
  assert.equal(semanticStatus.isError, false);
  assert.equal(semanticStatus.structuredContent.backends[0].state, "off");

  const semanticEnsure = await client.request("tools/call", {
    name: "ae.semantic.ensure",
    arguments: {
      language: "typescript",
      action: "warm"
    }
  });
  assert.equal(semanticEnsure.isError, false);
  assert.equal(semanticEnsure.structuredContent.backend.language, "typescript");
  assert.equal(semanticEnsure.structuredContent.backend.state, "ready");

  const semanticStatusAfterWarm = await client.request("tools/call", {
    name: "ae.semantic.status",
    arguments: {
      language: "typescript"
    }
  });
  assert.equal(semanticStatusAfterWarm.isError, false);
  assert.equal(semanticStatusAfterWarm.structuredContent.backends[0].state, "ready");

  const preparePlan = await client.request("tools/call", {
    name: "ae.edit.prepare_plan",
    arguments: {
      fileEdits: [
        {
          uri: pathToFileURL(path.join(context.workspaceDir, "nested", "example.ts")).toString(),
          edits: [
            {
              range: {
                start: { line: 0, character: 13 },
                end: { line: 0, character: 24 }
              },
              newText: "preparedValue"
            }
          ]
        }
      ]
    }
  });
  assert.equal(preparePlan.isError, false);
  assert.equal(preparePlan.structuredContent.summary.filesTouched, 1);
  assert.equal(preparePlan.structuredContent.summary.edits, 1);

  const applyPreparedPlan = await client.request("tools/call", {
    name: "ae.edit.apply_plan",
    arguments: {
      planId: preparePlan.structuredContent.planId,
      expectedWorkspaceRevision: preparePlan.structuredContent.workspaceRevision
    }
  });
  assert.equal(applyPreparedPlan.isError, false);
  assert.equal(applyPreparedPlan.structuredContent.editsApplied, 1);
  assert.equal(
    await readFile(path.join(context.workspaceDir, "nested", "example.ts"), "utf8"),
    "export const preparedValue = needle;\n"
  );

  const discard = await client.request("tools/call", {
    name: "ae.edit.discard_plan",
    arguments: {
      planId: "plan_missing"
    }
  });
  assert.equal(discard.isError, false);
  assert.equal(discard.structuredContent.discarded, false);

  const apply = await client.request("tools/call", {
    name: "ae.edit.apply_plan",
    arguments: {
      planId: "plan_missing",
      expectedWorkspaceRevision: "rev_1"
    }
  });
  assert.equal(apply.isError, true);
  assert.equal(apply.structuredContent.code, "AE_NOT_FOUND");

  const runningStatus = await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "status"], {
    cwd: projectRoot,
    env: context.env
  });
  assert.equal(runningStatus.code, 0, runningStatus.stderr);
  assert.equal(JSON.parse(runningStatus.stdout).reachable, true);

  const doctorAfter = await runProcess(process.execPath, ["./bin/spiki.js", "doctor"], {
    cwd: projectRoot,
    env: context.env
  });
  assert.equal(doctorAfter.code, 0, doctorAfter.stderr);
  assert.match(doctorAfter.stdout, /reachable: true/);

  await client.close();

  const stopResult = await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "stop"], {
    cwd: projectRoot,
    env: context.env
  });
  assert.equal(stopResult.code, 0, stopResult.stderr);
  assert.equal(JSON.parse(stopResult.stdout).stopped, true);

  const finalStatus = await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "status"], {
    cwd: projectRoot,
    env: context.env
  });
  assert.equal(finalStatus.code, 0, finalStatus.stderr);
  assert.equal(JSON.parse(finalStatus.stdout).reachable, false);
});
