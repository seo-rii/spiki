import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

import { createTestEnvironment, projectRoot, runProcess } from "./lib/test-env.mjs";

const RELATED_TASK_META_KEY = "io.modelcontextprotocol/related-task";

class McpLauncherClient {
  constructor(child, rootUri, options = {}) {
    this.child = child;
    this.rootUri = rootUri;
    this.options = options;
    this.buffer = Buffer.alloc(0);
    this.pending = new Map();
    this.notifications = [];
    this.deferredRootsList = null;
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

  request(method, params = {}, options = {}) {
    const id = String(options.id ?? this.nextId++);
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
      }, options.timeoutMs ?? 5000);

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

  deferNextRootsList() {
    if (this.deferredRootsList) {
      throw new Error("roots/list deferral already armed");
    }
    let resolveRequest;
    const request = new Promise((resolve) => {
      resolveRequest = resolve;
    });
    this.deferredRootsList = {
      message: null,
      resolveRequest
    };
    return request;
  }

  releaseDeferredRootsList() {
    if (!this.deferredRootsList?.message) {
      throw new Error("no deferred roots/list request to release");
    }
    this.replyToRootsList(this.deferredRootsList.message);
    this.deferredRootsList = null;
  }

  replyToRootsList(message) {
    this.send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        roots: [{ uri: this.rootUri, name: "integration" }]
      }
    });
  }

  handleMessage(message) {
    if (message.method === "roots/list") {
      if (this.deferredRootsList) {
        this.deferredRootsList.message = message;
        this.deferredRootsList.resolveRequest(message);
        return;
      }
      this.replyToRootsList(message);
      return;
    }

    if (message.method) {
      this.notifications.push(message);
      if (message.id == null) {
        return;
      }
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
      const error = new Error(message.error.message);
      error.code = message.error.code;
      pending.reject(error);
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
  const toolsList = await client.request("tools/list");
  client.notify("notifications/initialized");

  assert.equal(initialize.serverInfo.name, "spiki");
  assert.equal(initialize.serverInfo.title, "spiki");
  assert.equal(initialize.serverInfo.websiteUrl, "https://github.com/seo-rii/spiki");
  assert.deepEqual(initialize.capabilities.experimental.spikiPluginScaffold.clients, ["codex", "claude"]);
  assert.equal(workspaceStatus.isError, false);
  assert.equal(workspaceStatus.structuredContent.workspaceRevision, "rev_1");

  const workspaceTool = toolsList.tools.find((tool) => tool.name === "ae.workspace.status");
  const applyTool = toolsList.tools.find((tool) => tool.name === "ae.edit.apply_plan");
  assert.ok(workspaceTool, `missing workspace status tool in ${JSON.stringify(toolsList)}`);
  assert.ok(applyTool, `missing apply plan tool in ${JSON.stringify(toolsList)}`);
  assert.equal(workspaceTool.annotations.readOnlyHint, true);
  assert.equal(workspaceTool.annotations.openWorldHint, false);
  assert.equal(workspaceTool.outputSchema.properties.workspaceRevision.type, "string");
  assert.equal(applyTool.annotations.destructiveHint, true);
});

test("spiki launcher refuses to spawn over a live unreachable daemon pid", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-live-pid-guard-",
    files: {
      "index.ts": "const answer = 42;\n"
    }
  });
  const blocker = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000);"], {
    cwd: projectRoot,
    env: context.env,
    stdio: "ignore"
  });
  t.after(async () => {
    if (blocker.exitCode === null && blocker.signalCode === null) {
      blocker.kill("SIGTERM");
      await new Promise((resolve) => blocker.once("exit", resolve));
    }
    await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "stop"], {
      cwd: projectRoot,
      env: context.env,
      timeoutMs: 5000
    }).catch(() => {});
    await context.cleanup();
  });

  assert.equal(typeof blocker.pid, "number");
  await writeFile(path.join(context.runtimeDir, "daemon.pid"), `${blocker.pid}\n`);

  const guardScript = [
    'import { ensureDaemonRunning } from "./launcher/daemon-bootstrap.mjs";',
    "try {",
    "  await ensureDaemonRunning();",
    '  console.error("unexpected success");',
    "  process.exit(1);",
    "} catch (error) {",
    "  const message = error instanceof Error ? error.message : String(error);",
    '  if (!message.includes("Refusing to spawn a second spiki daemon")) {',
    "    console.error(message);",
    "    process.exit(2);",
    "  }",
    "}"
  ].join("\n");
  const result = await runProcess(
    process.execPath,
    ["--input-type=module", "-e", guardScript],
    {
      cwd: projectRoot,
      env: context.env,
      timeoutMs: 10000
    }
  );

  assert.equal(result.code, 0, `stdout:\n${result.stdout}\nstderr:\n${result.stderr}`);
});

test("spiki launcher rejects roots-less initialize by default", { timeout: 60000, concurrency: false }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-rootless-reject-",
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

  await assert.rejects(
    client.request("initialize", {
      protocolVersion: "2025-11-25",
      capabilities: {},
      clientInfo: {
        name: "spiki-rootless-test",
        version: "0.1.0"
      }
    }),
    /SPIKI_ALLOW_CWD_ROOT_FALLBACK/
  );
});

test("spiki launcher rejects initialize with an explicit empty root set", { timeout: 60000, concurrency: false }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-empty-roots-reject-",
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

  await assert.rejects(
    client.request("initialize", {
      protocolVersion: "2025-11-25",
      roots: [],
      capabilities: {
        roots: {
          listChanged: true
        }
      },
      clientInfo: {
        name: "spiki-empty-roots-test",
        version: "0.1.0"
      }
    }),
    (error) => error?.code === -32602 && /roots must not be empty/u.test(error.message)
  );
});

test("spiki launcher negotiates the server protocol version during initialize", {
  timeout: 60000,
  concurrency: false
}, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-protocol-negotiate-",
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

  const initialize = await client.request("initialize", {
    protocolVersion: "2024-10-07",
    roots: [{ uri: context.rootUri, name: "integration" }],
    capabilities: {
      roots: {
        listChanged: true
      }
    },
    clientInfo: {
      name: "spiki-protocol-negotiate-test",
      version: "0.1.0"
    }
  });
  client.notify("notifications/initialized");

  assert.equal(initialize.protocolVersion, "2025-11-25");
  assert.equal(initialize.capabilities.tools.listChanged, false);
  assert.deepEqual(initialize.capabilities.tasks, {
    list: {},
    cancel: {},
    requests: {
      tools: {
        call: {}
      }
    }
  });
  assert.deepEqual(initialize.capabilities.experimental.spikiPluginScaffold.clients, ["codex", "claude"]);
});

test("spiki launcher can allow roots-less initialize with explicit opt-in", { timeout: 60000, concurrency: false }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-rootless-optin-",
    files: {
      "index.ts": "const answer = 42;\n"
    }
  });
  const env = {
    ...context.env,
    SPIKI_ALLOW_CWD_ROOT_FALLBACK: "1"
  };
  t.after(async () => {
    await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "stop"], {
      cwd: projectRoot,
      env,
      timeoutMs: 5000
    }).catch(() => {});
    await context.cleanup();
  });

  const child = spawn(process.execPath, [path.join(projectRoot, "bin", "spiki.js")], {
    cwd: context.workspaceDir,
    env,
    stdio: ["pipe", "pipe", "inherit"]
  });
  const client = new McpLauncherClient(child, context.rootUri);
  t.after(async () => {
    await client.close().catch(() => {});
  });

  const initialize = await client.request("initialize", {
    protocolVersion: "2025-11-25",
    capabilities: {},
    clientInfo: {
      name: "spiki-rootless-optin-test",
      version: "0.1.0"
    }
  });
  client.notify("notifications/initialized");

  assert.equal(initialize.serverInfo.name, "spiki");

  const workspaceStatus = await client.request("tools/call", {
    name: "ae.workspace.status",
    arguments: {}
  });
  assert.equal(workspaceStatus.isError, false);
  assert.equal(workspaceStatus.structuredContent.roots[0], context.rootUri);
});

test("spiki launcher exits cleanly on malformed content-length input", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-malformed-bridge-",
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

  const result = await runProcess(process.execPath, ["./bin/spiki.js"], {
    cwd: projectRoot,
    env: context.env,
    stdinText: "content-length: 1\r\n\r\n{",
    timeoutMs: 10000
  });

  assert.equal(result.code, 1);
  assert.match(result.stderr, /Invalid JSON from client/);
});

test("spiki launcher exits cleanly on oversized client frames", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-oversized-bridge-",
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

  const result = await runProcess(process.execPath, ["./bin/spiki.js"], {
    cwd: projectRoot,
    env: context.env,
    stdinText: "content-length: 1048577\r\n\r\n",
    timeoutMs: 10000
  });

  assert.equal(result.code, 1);
  assert.match(result.stderr, /client frame exceeds 1048576 bytes/);
});

test("spiki launcher emits progress notifications for tool calls with progress tokens", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-progress-",
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

  const child = spawn(process.execPath, ["./bin/spiki.js"], {
    cwd: projectRoot,
    env: context.env,
    stdio: ["pipe", "pipe", "inherit"]
  });
  const client = new McpLauncherClient(child, context.rootUri);
  t.after(async () => {
    await client.close().catch(() => {});
  });

  await client.initialize();
  const notificationOffset = client.notifications.length;
  const search = await client.request("tools/call", {
    name: "ae.workspace.search_text",
    arguments: {
      query: "needle",
      mode: "literal",
      limit: 10
    },
    _meta: {
      progressToken: "progress-search"
    }
  });
  await new Promise((resolve) => setTimeout(resolve, 50));

  assert.equal(search.isError, false);
  const progressNotifications = client.notifications
    .slice(notificationOffset)
    .filter(
      (message) =>
        message.method === "notifications/progress" &&
        message.params?.progressToken === "progress-search"
    );
  assert.deepEqual(
    progressNotifications.map((message) => message.params.progress),
    [1, 2, 3]
  );
  assert.deepEqual(
    progressNotifications.map((message) => message.params.total),
    [3, 3, 3]
  );
  assert.match(progressNotifications[0].params.message, /Resolving workspace view/u);
  assert.match(progressNotifications[1].params.message, /Running ae\.workspace\.search_text/u);
  assert.match(progressNotifications[2].params.message, /Completed ae\.workspace\.search_text/u);
});

test("spiki launcher supports task-augmented search_text requests", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-task-search-",
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

  const child = spawn(process.execPath, ["./bin/spiki.js"], {
    cwd: projectRoot,
    env: context.env,
    stdio: ["pipe", "pipe", "inherit"]
  });
  const client = new McpLauncherClient(child, context.rootUri);
  t.after(async () => {
    await client.close().catch(() => {});
  });

  await client.initialize();
  const notificationOffset = client.notifications.length;
  const createTask = await client.request("tools/call", {
    name: "ae.workspace.search_text",
    arguments: {
      query: "needle",
      mode: "literal",
      limit: 10
    },
    task: {
      ttl: 60000
    },
    _meta: {
      progressToken: "task-search-progress"
    }
  });
  const taskId = createTask.task.taskId;

  assert.equal(createTask.task.status, "working");
  assert.equal(createTask._meta[RELATED_TASK_META_KEY].taskId, taskId);

  const listedTasks = await client.request("tasks/list", {});
  assert.ok(listedTasks.tasks.some((task) => task.taskId === taskId));

  const taskState = await client.request("tasks/get", { taskId });
  assert.ok(["working", "completed"].includes(taskState.status));

  const taskResult = await client.request("tasks/result", { taskId });
  assert.equal(taskResult.isError, false);
  assert.equal(taskResult.structuredContent.matches.length, 3);
  assert.equal(taskResult._meta[RELATED_TASK_META_KEY].taskId, taskId);

  await new Promise((resolve) => setTimeout(resolve, 50));
  const taskNotifications = client.notifications
    .slice(notificationOffset)
    .filter(
      (message) =>
        message.method === "notifications/tasks/status" && message.params?.taskId === taskId
    );
  assert.ok(taskNotifications.some((message) => message.params.status === "working"));
  assert.ok(taskNotifications.some((message) => message.params.status === "completed"));

  const progressNotifications = client.notifications
    .slice(notificationOffset)
    .filter(
      (message) =>
        message.method === "notifications/progress" &&
        message.params?.progressToken === "task-search-progress"
    );
  assert.ok(progressNotifications.length >= 2);
  assert.equal(progressNotifications[0].params._meta[RELATED_TASK_META_KEY].taskId, taskId);
});

test("spiki launcher rejects out-of-range task ttl values", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-task-ttl-",
    files: {
      "index.ts": "const needle = 1;\n"
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

  await client.initialize();

  await assert.rejects(
    client.request("tools/call", {
      name: "ae.workspace.search_text",
      arguments: {
        query: "needle",
        mode: "literal",
        limit: 10
      },
      task: {
        ttl: 0
      }
    }),
    (error) =>
      error?.code === -32602 &&
      /task\.ttl must be between 1000 and 3600000 milliseconds/u.test(error.message)
  );

  await assert.rejects(
    client.request("tools/call", {
      name: "ae.workspace.search_text",
      arguments: {
        query: "needle",
        mode: "literal",
        limit: 10
      },
      task: {
        ttl: 3_600_001
      }
    }),
    (error) =>
      error?.code === -32602 &&
      /task\.ttl must be between 1000 and 3600000 milliseconds/u.test(error.message)
  );
});

test("spiki launcher supports cancelling task-augmented requests", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-task-cancel-",
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

  await client.initialize();
  const deferredRootsList = client.deferNextRootsList();
  client.notify("notifications/roots/list_changed");

  const createTask = await client.request("tools/call", {
    name: "ae.workspace.search_text",
    arguments: {
      query: "answer",
      mode: "literal",
      limit: 10
    },
    task: {
      ttl: 60000
    }
  });
  const taskId = createTask.task.taskId;
  await deferredRootsList;

  const inFlightTask = await client.request("tasks/get", { taskId });
  assert.equal(inFlightTask.status, "working");

  const cancelledTask = await client.request("tasks/cancel", { taskId });
  assert.equal(cancelledTask.status, "cancelled");

  const taskResult = await client.request("tasks/result", { taskId });
  assert.equal(taskResult.isError, true);
  assert.equal(taskResult.structuredContent.code, "AE_CANCELLED");
  assert.equal(taskResult._meta[RELATED_TASK_META_KEY].taskId, taskId);

  client.releaseDeferredRootsList();
  await new Promise((resolve) => setTimeout(resolve, 50));
  const cancelledState = await client.request("tasks/get", { taskId });
  assert.equal(cancelledState.status, "cancelled");
});

test("spiki launcher suppresses responses for cancelled queued tool requests", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-cancel-queued-",
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

  await client.initialize();
  const deferredRootsList = client.deferNextRootsList();
  client.notify("notifications/roots/list_changed");

  const blockingRequest = client.request(
    "tools/call",
    {
      name: "ae.workspace.status",
      arguments: {
        includeCoverage: true
      }
    },
    { id: "blocking-request" }
  );
  await deferredRootsList;

  const cancelledRequest = client.request(
    "tools/call",
    {
      name: "ae.workspace.search_text",
      arguments: {
        query: "answer",
        mode: "literal",
        limit: 10
      }
    },
    { id: "cancelled-request", timeoutMs: 1000 }
  );
  client.notify("notifications/cancelled", {
    requestId: "cancelled-request",
    reason: "integration test"
  });
  await new Promise((resolve) => setTimeout(resolve, 50));

  client.releaseDeferredRootsList();

  const blockingResult = await blockingRequest;
  assert.equal(blockingResult.isError, false);
  await assert.rejects(cancelledRequest, /Timed out waiting for tools\/call/u);
});

test("spiki CLI and launcher bridge manage daemon lifecycle", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-program-",
    files: {
      "index.ts": "const needle = 1;\nconsole.log(needle);\n",
      "nested/example.ts": "export const nestedValue = needle;\n",
      "dist/generated.ts": "export const generatedValue = needle;\n"
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
  assert.equal(initialize.capabilities.tools.listChanged, false);
  assert.deepEqual(initialize.capabilities.tasks, {
    list: {},
    cancel: {},
    requests: {
      tools: {
        call: {}
      }
    }
  });
  assert.deepEqual(initialize.capabilities.experimental.spikiPluginScaffold.clients, ["codex", "claude"]);
  if (process.platform !== "win32") {
    const runtimeDirStat = await stat(context.runtimeDir);
    const pidFileStat = await stat(path.join(context.runtimeDir, "daemon.pid"));
    assert.equal(runtimeDirStat.mode & 0o777, 0o700);
    assert.equal(pidFileStat.mode & 0o777, 0o600);
  }

  const runningStatusAfterInit = await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "status"], {
    cwd: projectRoot,
    env: context.env
  });
  assert.equal(runningStatusAfterInit.code, 0, runningStatusAfterInit.stderr);
  const runningStatusJson = JSON.parse(runningStatusAfterInit.stdout);
  assert.equal(runningStatusJson.reachable, true);
  assert.equal(runningStatusJson.compatible, true);
  await assert.rejects(
    client.request("resources/list"),
    (error) => error?.code === -32601 && /method not found: resources\/list/u.test(error.message)
  );
  await assert.rejects(
    client.request("resources/templates/list"),
    (error) =>
      error?.code === -32601 && /method not found: resources\/templates\/list/u.test(error.message)
  );

  const tools = await client.request("tools/list");
  const toolNames = tools.tools.map((tool) => tool.name).sort();
  assert.deepEqual(toolNames, [
    "ae.edit.apply_plan",
    "ae.edit.discard_plan",
    "ae.edit.inspect_plan",
    "ae.edit.prepare_plan",
    "ae.semantic.ensure",
    "ae.semantic.status",
    "ae.symbol.definition",
    "ae.workspace.read_spans",
    "ae.workspace.search_text",
    "ae.workspace.status"
  ]);
  assert.equal(
    tools.tools.find((tool) => tool.name === "ae.workspace.search_text").execution.taskSupport,
    "optional"
  );

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

  const searchIncludingDefaultExcluded = await client.request("tools/call", {
    name: "ae.workspace.search_text",
    arguments: {
      query: "needle",
      mode: "literal",
      limit: 10,
      scope: {
        includeDefaultExcluded: true
      }
    }
  });
  assert.equal(searchIncludingDefaultExcluded.isError, false);
  assert.equal(searchIncludingDefaultExcluded.structuredContent.matches.length, 4);

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

  const invalidSemanticStatus = await client.request("tools/call", {
    name: "ae.semantic.status",
    arguments: {
      language: 42,
      extra: true
    }
  });
  assert.equal(invalidSemanticStatus.isError, true);
  assert.equal(invalidSemanticStatus.structuredContent.code, "AE_INVALID_REQUEST");
  assert.match(invalidSemanticStatus.structuredContent.message, /language|extra/);

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

  const inspectPlan = await client.request("tools/call", {
    name: "ae.edit.inspect_plan",
    arguments: {
      planId: preparePlan.structuredContent.planId
    }
  });
  assert.equal(inspectPlan.isError, false);
  assert.equal(inspectPlan.structuredContent.planId, preparePlan.structuredContent.planId);
  assert.equal(inspectPlan.structuredContent.fileEdits.length, 1);
  assert.equal(inspectPlan.structuredContent.summary.edits, 1);

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

  const prepareDiscardPlan = await client.request("tools/call", {
    name: "ae.edit.prepare_plan",
    arguments: {
      fileEdits: [
        {
          uri: pathToFileURL(path.join(context.workspaceDir, "nested", "example.ts")).toString(),
          edits: [
            {
              range: {
                start: { line: 0, character: 13 },
                end: { line: 0, character: 26 }
              },
              newText: "discardedValue"
            }
          ]
        }
      ]
    }
  });
  assert.equal(prepareDiscardPlan.isError, false);

  const discardPreparedPlan = await client.request("tools/call", {
    name: "ae.edit.discard_plan",
    arguments: {
      planId: prepareDiscardPlan.structuredContent.planId
    }
  });
  assert.equal(discardPreparedPlan.isError, false);
  assert.equal(discardPreparedPlan.structuredContent.discarded, true);
  assert.equal(
    await readFile(path.join(context.workspaceDir, "nested", "example.ts"), "utf8"),
    "export const preparedValue = needle;\n"
  );

  const inspectDiscardedPlan = await client.request("tools/call", {
    name: "ae.edit.inspect_plan",
    arguments: {
      planId: prepareDiscardPlan.structuredContent.planId
    }
  });
  assert.equal(inspectDiscardedPlan.isError, true);
  assert.equal(inspectDiscardedPlan.structuredContent.code, "AE_NOT_FOUND");

  const discardMissingPlan = await client.request("tools/call", {
    name: "ae.edit.discard_plan",
    arguments: {
      planId: "plan_missing"
    }
  });
  assert.equal(discardMissingPlan.isError, false);
  assert.equal(discardMissingPlan.structuredContent.discarded, false);

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

async function createSemanticLauncherSession(t, options = {}) {
  const fakeLspPath = path.join(projectRoot, "tests", "lib", "fake-lsp-server.mjs");
  const serverArgs = options.serverArgs ?? [];
  const context = await createTestEnvironment({
    prefix: "spiki-semantic-",
    files: {
      "src/index.ts": [
        "export const answer = 42;",
        "export function useAnswer() {",
        "  return answer;",
        "}",
        ""
      ].join("\n"),
      "package.json": JSON.stringify(
        {
          dependencies: {
            typescript: "5.8.0"
          }
        },
        null,
        2
      ),
      "spiki.languages.yaml": [
        "bindings:",
        "  typescript:",
        "    kind: lsp",
        "    provider: fake-typescript",
        `    command: ${JSON.stringify(process.execPath)}`,
        "    args:",
        `      - ${JSON.stringify(fakeLspPath)}`,
        ...serverArgs.map((arg) => `      - ${JSON.stringify(arg)}`)
      ].join("\n")
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

  await client.initialize();
  return {
    client,
    context,
    definitionUri: pathToFileURL(path.join(context.workspaceDir, "src", "index.ts")).toString()
  };
}

test("spiki launcher supports configured lsp definition requests", { timeout: 60000 }, async (t) => {
  const { client, context, definitionUri } = await createSemanticLauncherSession(t);

  const statusBefore = await client.request("tools/call", {
    name: "ae.semantic.status",
    arguments: {
      language: "typescript"
    }
  });
  assert.equal(statusBefore.isError, false);
  assert.equal(statusBefore.structuredContent.backends[0].provider, "fake-typescript");
  assert.equal(statusBefore.structuredContent.backends[0].state, "off");

  const ensured = await client.request("tools/call", {
    name: "ae.semantic.ensure",
    arguments: {
      language: "typescript",
      action: "warm"
    }
  });
  assert.equal(ensured.isError, false);
  assert.equal(ensured.structuredContent.backend.provider, "fake-typescript");
  assert.equal(ensured.structuredContent.backend.state, "ready");

  const definition = await client.request("tools/call", {
    name: "ae.symbol.definition",
    arguments: {
      language: "typescript",
      uri: definitionUri,
      position: {
        line: 2,
        character: 10
      }
    }
  });
  assert.equal(definition.isError, false);
  assert.equal(definition.structuredContent.backend.provider, "fake-typescript");
  assert.equal(definition.structuredContent.definitions.length, 1);
  assert.equal(definition.structuredContent.definitions[0].uri, definitionUri);
  assert.deepEqual(definition.structuredContent.definitions[0].range, {
    start: { line: 0, character: 13 },
    end: { line: 0, character: 19 }
  });
});

test("spiki launcher surfaces configured lsp definition backend errors", { timeout: 60000 }, async (t) => {
  const { client, definitionUri } = await createSemanticLauncherSession(t, {
    serverArgs: ["error"]
  });

  const ensured = await client.request("tools/call", {
    name: "ae.semantic.ensure",
    arguments: {
      language: "typescript",
      action: "warm"
    }
  });
  assert.equal(ensured.isError, false);

  const definition = await client.request("tools/call", {
    name: "ae.symbol.definition",
    arguments: {
      language: "typescript",
      uri: definitionUri,
      position: {
        line: 2,
        character: 10
      }
    }
  });
  assert.equal(definition.isError, true);
  assert.equal(definition.structuredContent.code, "AE_SEMANTIC_ERROR");
  assert.match(definition.structuredContent.message, /textDocument\/definition/u);
  assert.match(definition.structuredContent.message, /Definition failed by test backend/u);
});

test("spiki launcher times out stalled lsp definition requests", { timeout: 60000 }, async (t) => {
  const { client, definitionUri } = await createSemanticLauncherSession(t, {
    serverArgs: ["timeout"]
  });

  const ensured = await client.request("tools/call", {
    name: "ae.semantic.ensure",
    arguments: {
      language: "typescript",
      action: "warm"
    }
  });
  assert.equal(ensured.isError, false);

  const definition = await client.request("tools/call", {
    name: "ae.symbol.definition",
    arguments: {
      language: "typescript",
      uri: definitionUri,
      position: {
        line: 2,
        character: 10
      }
    }
  });
  assert.equal(definition.isError, true);
  assert.equal(definition.structuredContent.code, "AE_SEMANTIC_ERROR");
  assert.match(definition.structuredContent.message, /textDocument\/definition/u);
  assert.match(definition.structuredContent.message, /timed out after 2000ms/u);
});
