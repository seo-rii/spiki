import assert from "node:assert/strict";
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
  socket.end();
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
