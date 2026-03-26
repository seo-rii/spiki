import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { chmod, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import { daemonStatus, ensureDaemonRunning } from "../launcher/runtime.mjs";
import { createTestEnvironment } from "./lib/test-env.mjs";

test("ensureDaemonRunning serializes concurrent daemon bootstrap", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-bootstrap-race-"
  });
  const fakeDaemonPath = path.join(context.tempRoot, "fake-daemon.mjs");
  const spawnLogPath = path.join(context.tempRoot, "spawn-log.txt");

  await writeFile(
    fakeDaemonPath,
    `#!/usr/bin/env node
import { appendFile, mkdir, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";

const args = process.argv.slice(2);
const socketPath = args[args.indexOf("--socket") + 1];
const runtimeDir = args[args.indexOf("--runtime-dir") + 1];
const delayMs = Number(process.env.SPIKI_TEST_DAEMON_DELAY_MS ?? "400");
const spawnLogPath = process.env.SPIKI_TEST_SPAWN_LOG;

await mkdir(runtimeDir, { recursive: true });
await appendFile(spawnLogPath, String(process.pid) + "\\n");
await writeFile(path.join(runtimeDir, "daemon.pid"), String(process.pid) + "\\n");
await new Promise((resolve) => setTimeout(resolve, delayMs));
await rm(socketPath, { force: true });

const server = net.createServer((socket) => {
  let buffer = Buffer.alloc(0);
  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (true) {
      const headerEnd = buffer.indexOf("\\r\\n\\r\\n");
      if (headerEnd === -1) {
        return;
      }

      const header = buffer.subarray(0, headerEnd).toString("utf8");
      const contentLengthLine = header
        .split(/\\r?\\n/u)
        .find((line) => line.toLowerCase().startsWith("content-length:"));
      const length = Number(contentLengthLine.split(":")[1].trim());
      const bodyStart = headerEnd + 4;
      if (buffer.length < bodyStart + length) {
        return;
      }

      const payload = buffer.subarray(bodyStart, bodyStart + length);
      buffer = buffer.subarray(bodyStart + length);
      const message = JSON.parse(payload.toString("utf8"));
      const response = Buffer.from(
        JSON.stringify({
          jsonrpc: "2.0",
          id: message.id,
          result: {
            serverInfo: {
              name: "spiki",
              version: "0.1.0-dev"
            },
            protocolVersion: "2025-11-25",
            bootstrapVersion: 1
          }
        }),
        "utf8"
      );
      socket.write(\`Content-Length: \${response.length}\\r\\n\\r\\n\`);
      socket.write(response);
    }
  });
});

const shutdown = async () => {
  await rm(socketPath, { force: true }).catch(() => {});
  server.close(() => {
    process.exit(0);
  });
};

process.on("SIGTERM", () => {
  shutdown().catch(() => {
    process.exit(1);
  });
});

process.on("SIGINT", () => {
  shutdown().catch(() => {
    process.exit(1);
  });
});

server.listen(socketPath);
`
  );
  await chmod(fakeDaemonPath, 0o755);

  const previousEnv = new Map([
    ["AGENT_EDITOR_RUNTIME_DIR", process.env.AGENT_EDITOR_RUNTIME_DIR],
    ["SPIKI_RUNTIME_DIR", process.env.SPIKI_RUNTIME_DIR],
    ["SPIKI_DAEMON_BIN", process.env.SPIKI_DAEMON_BIN],
    ["SPIKI_TEST_DAEMON_DELAY_MS", process.env.SPIKI_TEST_DAEMON_DELAY_MS],
    ["SPIKI_TEST_SPAWN_LOG", process.env.SPIKI_TEST_SPAWN_LOG]
  ]);
  process.env.AGENT_EDITOR_RUNTIME_DIR = context.runtimeDir;
  delete process.env.SPIKI_RUNTIME_DIR;
  process.env.SPIKI_DAEMON_BIN = fakeDaemonPath;
  process.env.SPIKI_TEST_DAEMON_DELAY_MS = "400";
  process.env.SPIKI_TEST_SPAWN_LOG = spawnLogPath;
  await rm(context.runtimeDir, { recursive: true, force: true });

  t.after(async () => {
    try {
      const pidLog = await readFile(spawnLogPath, "utf8").catch(() => "");
      for (const value of pidLog.split("\n")) {
        const pid = Number(value.trim());
        if (!Number.isInteger(pid) || pid <= 0) {
          continue;
        }

        try {
          process.kill(pid, "SIGTERM");
        } catch {}
      }

      await new Promise((resolve) => setTimeout(resolve, 250));

      for (const value of pidLog.split("\n")) {
        const pid = Number(value.trim());
        if (!Number.isInteger(pid) || pid <= 0) {
          continue;
        }

        try {
          process.kill(pid, "SIGKILL");
        } catch {}
      }
    } finally {
      for (const [key, value] of previousEnv.entries()) {
        if (value === undefined) {
          delete process.env[key];
        } else {
          process.env[key] = value;
        }
      }

      await context.cleanup();
    }
  });

  const [first, second] = await Promise.all([ensureDaemonRunning(), ensureDaemonRunning()]);
  assert.equal(first.socketPath, second.socketPath);

  const status = await daemonStatus();
  assert.equal(status.reachable, true);

  const pidLog = await readFile(spawnLogPath, "utf8");
  const spawnedPids = pidLog
    .split("\n")
    .map((value) => value.trim())
    .filter(Boolean);
  assert.equal(spawnedPids.length, 1);
});

test("ensureDaemonRunning replaces an incompatible reachable daemon", { timeout: 60000 }, async (t) => {
  const context = await createTestEnvironment({
    prefix: "spiki-bootstrap-version-"
  });
  const fakeDaemonPath = path.join(context.tempRoot, "incompatible-daemon.mjs");
  const previousEnv = new Map([
    ["AGENT_EDITOR_RUNTIME_DIR", process.env.AGENT_EDITOR_RUNTIME_DIR],
    ["SPIKI_RUNTIME_DIR", process.env.SPIKI_RUNTIME_DIR]
  ]);

  await writeFile(
    fakeDaemonPath,
    `#!/usr/bin/env node
import { mkdir, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";

const args = process.argv.slice(2);
const socketPath = args[args.indexOf("--socket") + 1];
const runtimeDir = args[args.indexOf("--runtime-dir") + 1];

await mkdir(runtimeDir, { recursive: true });
await writeFile(path.join(runtimeDir, "daemon.pid"), String(process.pid) + "\\n");
await rm(socketPath, { force: true });

const respond = (socket, message) => {
  const payload = Buffer.from(
    JSON.stringify({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        serverInfo: {
          name: "spiki",
          version: "0.0.0-stale"
        },
        protocolVersion: "2025-11-25",
        bootstrapVersion: 0
      }
    }),
    "utf8"
  );
  socket.write(\`Content-Length: \${payload.length}\\r\\n\\r\\n\`);
  socket.write(payload);
};

const server = net.createServer((socket) => {
  let buffer = Buffer.alloc(0);
  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (true) {
      const headerEnd = buffer.indexOf("\\r\\n\\r\\n");
      if (headerEnd === -1) {
        return;
      }

      const header = buffer.subarray(0, headerEnd).toString("utf8");
      const contentLengthLine = header
        .split(/\\r?\\n/u)
        .find((line) => line.toLowerCase().startsWith("content-length:"));
      const length = Number(contentLengthLine.split(":")[1].trim());
      const bodyStart = headerEnd + 4;
      if (buffer.length < bodyStart + length) {
        return;
      }

      const payload = buffer.subarray(bodyStart, bodyStart + length);
      buffer = buffer.subarray(bodyStart + length);
      respond(socket, JSON.parse(payload.toString("utf8")));
    }
  });
});

const shutdown = async () => {
  await rm(socketPath, { force: true }).catch(() => {});
  server.close(() => {
    process.exit(0);
  });
};

process.on("SIGTERM", () => {
  shutdown().catch(() => {
    process.exit(1);
  });
});

process.on("SIGINT", () => {
  shutdown().catch(() => {
    process.exit(1);
  });
});

server.listen(socketPath, () => {
  process.stdout.write("ready\\n");
});
`
  );
  await chmod(fakeDaemonPath, 0o755);
  process.env.AGENT_EDITOR_RUNTIME_DIR = context.runtimeDir;
  delete process.env.SPIKI_RUNTIME_DIR;

  const fakeDaemon = spawn(process.execPath, [fakeDaemonPath, "--socket", path.join(context.runtimeDir, "daemon.sock"), "--runtime-dir", context.runtimeDir], {
    cwd: context.workspaceDir,
    env: process.env,
    stdio: ["ignore", "pipe", "inherit"]
  });
  const fakeExit = new Promise((resolve) => {
    fakeDaemon.once("exit", (code, signal) => resolve({ code, signal }));
  });
  await new Promise((resolve, reject) => {
    fakeDaemon.stdout.once("data", (chunk) => {
      if (chunk.toString("utf8").includes("ready")) {
        resolve();
        return;
      }
      reject(new Error(`unexpected fake daemon readiness output: ${chunk.toString("utf8")}`));
    });
    fakeDaemon.once("error", reject);
    fakeDaemon.once("exit", (code) => reject(new Error(`fake daemon exited early with code ${code}`)));
  });

  t.after(async () => {
    try {
      if (fakeDaemon.exitCode === null && fakeDaemon.signalCode === null) {
        fakeDaemon.kill("SIGTERM");
      }
      await daemonStatus()
        .then((status) => {
          if (status.pid && Number.isInteger(status.pid)) {
            try {
              process.kill(status.pid, "SIGTERM");
            } catch {}
          }
        })
        .catch(() => {});
      await new Promise((resolve) => setTimeout(resolve, 250));
    } finally {
      for (const [key, value] of previousEnv.entries()) {
        if (value === undefined) {
          delete process.env[key];
        } else {
          process.env[key] = value;
        }
      }
      await context.cleanup();
    }
  });

  const result = await ensureDaemonRunning();
  const status = await daemonStatus();
  const fakeResult = await fakeExit;

  assert.equal(result.runtimeDir, context.runtimeDir);
  assert.equal(status.reachable, true);
  assert.equal(status.compatible, true);
  assert.notEqual(status.pid, fakeDaemon.pid);
  assert.notEqual(fakeResult.code, null);
});
