import { spawn, spawnSync } from "node:child_process";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import fs from "node:fs/promises";

function getProjectRoot() {
  return path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
}

function getRuntimeDir() {
  const runtimeDir =
    process.env.AGENT_EDITOR_RUNTIME_DIR ??
    process.env.SPIKI_RUNTIME_DIR ??
    path.join(os.tmpdir(), `spiki-${process.getuid?.() ?? os.userInfo().username}`);

  return runtimeDir;
}

function getSocketPath(runtimeDir) {
  if (process.platform === "win32") {
    const user = (process.env.USERNAME ?? os.userInfo().username).replaceAll(/[^a-zA-Z0-9_-]/g, "_");
    return `\\\\.\\pipe\\spiki-${user}`;
  }

  return path.join(runtimeDir, "daemon.sock");
}

async function ensureRuntimeDir(runtimeDir) {
  if (process.platform !== "win32") {
    await fs.mkdir(runtimeDir, { recursive: true });
  }
}

async function removeIfExists(targetPath) {
  try {
    await fs.rm(targetPath, { force: true });
  } catch (error) {
    if (error && error.code !== "ENOENT") {
      throw error;
    }
  }
}

function resolveDaemonBinary(projectRoot) {
  if (process.env.SPIKI_DAEMON_BIN) {
    return process.env.SPIKI_DAEMON_BIN;
  }

  const binaryName = process.platform === "win32" ? "spiki-daemon.exe" : "spiki-daemon";
  return path.join(projectRoot, "target", "debug", binaryName);
}

function connectSocket(socketPath, timeoutMs = 250) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(socketPath);
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error("Timed out connecting to spiki daemon"));
    }, timeoutMs);

    socket.once("connect", () => {
      clearTimeout(timer);
      resolve(socket);
    });

    socket.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

async function isDaemonReachable(socketPath, timeoutMs = 250) {
  try {
    const socket = await connectSocket(socketPath, timeoutMs);
    socket.destroy();
    return true;
  } catch {
    return false;
  }
}

function isProcessAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function readPid(runtimeDir) {
  try {
    const pidText = await fs.readFile(path.join(runtimeDir, "daemon.pid"), "utf8");
    const pid = Number(pidText.trim());
    return Number.isInteger(pid) && pid > 0 ? pid : null;
  } catch (error) {
    if (error && error.code === "ENOENT") {
      return null;
    }

    throw error;
  }
}

async function buildDaemonBinary(projectRoot) {
  const result = spawnSync(process.execPath, ["./scripts/build-daemon.mjs"], {
    cwd: projectRoot,
    stdio: ["ignore", "pipe", "pipe"],
    encoding: "utf8"
  });

  if (result.status !== 0) {
    const details = [result.stdout, result.stderr].filter(Boolean).join("\n").trim();
    throw new Error(details ? `Failed to build spiki daemon\n${details}` : "Failed to build spiki daemon");
  }
}

async function cleanupStaleRuntime(runtimeDir, socketPath) {
  const pid = await readPid(runtimeDir);
  if (pid && isProcessAlive(pid)) {
    return;
  }

  await removeIfExists(path.join(runtimeDir, "daemon.pid"));
  if (process.platform !== "win32") {
    await removeIfExists(socketPath);
  }
}

async function waitForDaemon(socketPath, timeoutMs = 5000) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if (await isDaemonReachable(socketPath, 250)) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  throw new Error("Timed out waiting for spiki daemon readiness");
}

export async function ensureDaemonRunning() {
  const projectRoot = getProjectRoot();
  const runtimeDir = getRuntimeDir();
  const socketPath = getSocketPath(runtimeDir);
  const daemonBin = resolveDaemonBinary(projectRoot);

  await ensureRuntimeDir(runtimeDir);

  if (await isDaemonReachable(socketPath, 250)) {
    return { projectRoot, runtimeDir, socketPath, daemonBin };
  }

  const lockPath = path.join(runtimeDir, "bootstrap.lock");
  const lockDeadline = Date.now() + 10000;
  while (true) {
    try {
      await fs.mkdir(lockPath);
      break;
    } catch (error) {
      if (!error || error.code !== "EEXIST") {
        throw error;
      }

      try {
        const lockStat = await fs.stat(lockPath);
        if (Date.now() - lockStat.mtimeMs > 30000) {
          await fs.rm(lockPath, { recursive: true, force: true });
          continue;
        }
      } catch (statError) {
        if (statError && statError.code === "ENOENT") {
          continue;
        }

        throw statError;
      }

      if (Date.now() >= lockDeadline) {
        throw new Error("Timed out waiting for spiki bootstrap lock");
      }

      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }

  try {
    await fs.writeFile(
      path.join(lockPath, "owner.json"),
      JSON.stringify({
        pid: process.pid,
        createdAt: new Date().toISOString()
      })
    );

    if (await isDaemonReachable(socketPath, 250)) {
      return { projectRoot, runtimeDir, socketPath, daemonBin };
    }

    await cleanupStaleRuntime(runtimeDir, socketPath);
    if (await isDaemonReachable(socketPath, 250)) {
      return { projectRoot, runtimeDir, socketPath, daemonBin };
    }

    try {
      await fs.access(daemonBin);
    } catch {
      await buildDaemonBinary(projectRoot);
    }

    const child = spawn(daemonBin, ["--socket", socketPath, "--runtime-dir", runtimeDir], {
      cwd: projectRoot,
      detached: true,
      stdio: "ignore",
      env: {
        ...process.env,
        RUST_LOG: process.env.RUST_LOG ?? "info"
      }
    });

    child.unref();
    await waitForDaemon(socketPath);
    return { projectRoot, runtimeDir, socketPath, daemonBin };
  } finally {
    await fs.rm(lockPath, { recursive: true, force: true });
  }
}

export async function bridgeStdio() {
  const { socketPath } = await ensureDaemonRunning();
  const socket = await connectSocket(socketPath, 1000);
  let finished = false;
  let clientMode = null;
  let stdinBuffer = Buffer.alloc(0);
  let socketBuffer = Buffer.alloc(0);
  const allowCwdRootFallback = process.env.SPIKI_ALLOW_CWD_ROOT_FALLBACK === "1";

  process.stdin.resume();
  const finish = (reject, error) => {
    if (finished) {
      return;
    }
    finished = true;
    if (error) {
      reject(error);
      return;
    }
    if (!socket.destroyed) {
      socket.destroy();
    }
  };

  process.stdin.once("end", () => {
    socket.end();
    setTimeout(() => {
      if (!finished) {
        socket.destroy();
      }
    }, 100).unref();
  });
  process.stdin.once("close", () => {
    socket.end();
  });
  process.stdin.on("data", (chunk) => {
    stdinBuffer = Buffer.concat([stdinBuffer, chunk]);

    while (stdinBuffer.length > 0) {
      if (!clientMode) {
        const preview = stdinBuffer.subarray(0, Math.min(stdinBuffer.length, 64)).toString("utf8");
        if (preview.trimStart().startsWith("{")) {
          clientMode = "jsonl";
        } else if (preview.toLowerCase().startsWith("content-length:")) {
          clientMode = "content-length";
        } else {
          return;
        }
      }

      if (clientMode === "content-length") {
        const headerEnd = stdinBuffer.indexOf("\r\n\r\n");
        if (headerEnd === -1) {
          return;
        }

        const header = stdinBuffer.subarray(0, headerEnd).toString("utf8");
        const contentLengthLine = header
          .split(/\r?\n/u)
          .find((line) => line.toLowerCase().startsWith("content-length:"));
        if (!contentLengthLine) {
          throw new Error(`Missing Content-Length header: ${header}`);
        }

        const length = Number(contentLengthLine.split(":")[1].trim());
        const bodyStart = headerEnd + 4;
        if (stdinBuffer.length < bodyStart + length) {
          return;
        }

        const payload = stdinBuffer.subarray(bodyStart, bodyStart + length);
        stdinBuffer = stdinBuffer.subarray(bodyStart + length);
        const message = JSON.parse(payload.toString("utf8"));

        if (
          message.method === "initialize" &&
          !message.params?.roots &&
          !message.params?.capabilities?.roots
        ) {
          if (!allowCwdRootFallback) {
            const response = {
              jsonrpc: "2.0",
              id: message.id ?? null,
              error: {
                code: -32602,
                message:
                  "Client must provide initialize.params.roots or set SPIKI_ALLOW_CWD_ROOT_FALLBACK=1"
              }
            };
            const responsePayload = Buffer.from(JSON.stringify(response), "utf8");
            process.stdout.write(`Content-Length: ${responsePayload.length}\r\n\r\n`);
            process.stdout.write(responsePayload);
            continue;
          }

          const params = message.params ?? {};
          message.params = {
            ...params,
            roots: [{ uri: pathToFileURL(process.cwd()).toString(), name: path.basename(process.cwd()) || "workspace" }]
          };
        }

        const forwardPayload = Buffer.from(JSON.stringify(message), "utf8");
        socket.write(`Content-Length: ${forwardPayload.length}\r\n\r\n`);
        socket.write(forwardPayload);
        continue;
      }

      const newlineIndex = stdinBuffer.indexOf("\n");
      if (newlineIndex === -1) {
        return;
      }

      const line = stdinBuffer.subarray(0, newlineIndex).toString("utf8").trim();
      stdinBuffer = stdinBuffer.subarray(newlineIndex + 1);
      if (line.length === 0) {
        continue;
      }

      const message = JSON.parse(line);
      if (
        message.method === "initialize" &&
        !message.params?.roots &&
        !message.params?.capabilities?.roots
      ) {
        if (!allowCwdRootFallback) {
          process.stdout.write(
            `${JSON.stringify({
              jsonrpc: "2.0",
              id: message.id ?? null,
              error: {
                code: -32602,
                message:
                  "Client must provide initialize.params.roots or set SPIKI_ALLOW_CWD_ROOT_FALLBACK=1"
              }
            })}\n`
          );
          continue;
        }

        const params = message.params ?? {};
        message.params = {
          ...params,
          roots: [{ uri: pathToFileURL(process.cwd()).toString(), name: path.basename(process.cwd()) || "workspace" }]
        };
      }

      const payload = Buffer.from(JSON.stringify(message), "utf8");
      socket.write(`Content-Length: ${payload.length}\r\n\r\n`);
      socket.write(payload);
    }
  });
  socket.on("data", (chunk) => {
    socketBuffer = Buffer.concat([socketBuffer, chunk]);

    while (true) {
      const headerEnd = socketBuffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) {
        return;
      }

      const header = socketBuffer.subarray(0, headerEnd).toString("utf8");
      const contentLengthLine = header
        .split(/\r?\n/u)
        .find((line) => line.toLowerCase().startsWith("content-length:"));
      if (!contentLengthLine) {
        throw new Error(`Missing Content-Length header from daemon: ${header}`);
      }

      const length = Number(contentLengthLine.split(":")[1].trim());
      const bodyStart = headerEnd + 4;
      if (socketBuffer.length < bodyStart + length) {
        return;
      }

      const payload = socketBuffer.subarray(bodyStart, bodyStart + length);
      socketBuffer = socketBuffer.subarray(bodyStart + length);
      const message = JSON.parse(payload.toString("utf8"));

      if (clientMode === "jsonl") {
        process.stdout.write(`${JSON.stringify(message)}\n`);
        continue;
      }

      process.stdout.write(`Content-Length: ${payload.length}\r\n\r\n`);
      process.stdout.write(payload);
    }
  });

  const shutdown = () => {
    socket.end();
  };

  process.once("SIGINT", shutdown);
  process.once("SIGTERM", shutdown);

  await new Promise((resolve, reject) => {
    socket.once("close", () => {
      if (finished) {
        resolve();
        return;
      }
      finished = true;
      resolve();
    });
    socket.once("error", (error) => finish(reject, error));
  });
}

export async function daemonStatus() {
  const runtimeDir = getRuntimeDir();
  const socketPath = getSocketPath(runtimeDir);
  const daemonBin = resolveDaemonBinary(getProjectRoot());
  const pid = await readPid(runtimeDir);
  const reachable = await isDaemonReachable(socketPath, 250);

  return {
    runtimeDir,
    socketPath,
    daemonBin,
    pid,
    reachable
  };
}

export async function stopDaemon() {
  const runtimeDir = getRuntimeDir();
  const socketPath = getSocketPath(runtimeDir);
  const pid = await readPid(runtimeDir);

  if (!pid || !isProcessAlive(pid)) {
    await cleanupStaleRuntime(runtimeDir, socketPath);
    return { stopped: false, reason: "not-running" };
  }

  process.kill(pid, "SIGTERM");
  let stopped = false;
  const startedAt = Date.now();
  while (Date.now() - startedAt < 3000) {
    if (!isProcessAlive(pid)) {
      stopped = true;
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  if (!stopped && isProcessAlive(pid)) {
    process.kill(pid, "SIGKILL");
    const forcedAt = Date.now();
    while (Date.now() - forcedAt < 2000) {
      if (!isProcessAlive(pid)) {
        stopped = true;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }

  await cleanupStaleRuntime(runtimeDir, socketPath);
  return stopped ? { stopped: true } : { stopped: false, reason: "force-stop-timeout" };
}

export async function runDoctor() {
  const status = await daemonStatus();
  const lines = [
    `runtimeDir: ${status.runtimeDir}`,
    `socketPath: ${status.socketPath}`,
    `daemonBin: ${status.daemonBin}`,
    `pid: ${status.pid ?? "missing"}`,
    `reachable: ${status.reachable}`
  ];

  process.stdout.write(`${lines.join("\n")}\n`);
}
