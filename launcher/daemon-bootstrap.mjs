import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import net from "node:net";
import path from "node:path";

import { getProjectRoot, getRuntimeDir, getSocketPath, resolveDaemonBinary } from "./runtime-paths.mjs";

const SPIKI_SERVER_NAME = "spiki";
const SPIKI_SERVER_VERSION = "0.1.0-dev";
const SPIKI_PROTOCOL_VERSION = "2025-11-25";
const SPIKI_BOOTSTRAP_VERSION = 1;

async function ensureRuntimeDir(runtimeDir) {
  await fs.mkdir(runtimeDir, { recursive: true, mode: 0o700 });
  const runtimeStat = await fs.lstat(runtimeDir);
  if (!runtimeStat.isDirectory() || runtimeStat.isSymbolicLink()) {
    throw new Error(`spiki runtime path is not a real directory: ${runtimeDir}`);
  }
  if (process.platform !== "win32") {
    await fs.chmod(runtimeDir, 0o700);
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

export function connectSocket(socketPath, timeoutMs = 250) {
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

async function probeDaemonStatus(socketPath, timeoutMs = 250) {
  let socket;
  try {
    socket = await connectSocket(socketPath, timeoutMs);
  } catch (error) {
    return {
      reachable: false,
      compatible: false,
      reason: error instanceof Error ? error.message : String(error),
      serverInfo: null,
      protocolVersion: null,
      bootstrapVersion: null
    };
  }

  return await new Promise((resolve) => {
    let settled = false;
    let buffer = Buffer.alloc(0);
    const requestPayload = Buffer.from(
      JSON.stringify({
        jsonrpc: "2.0",
        id: "bootstrap",
        method: "spiki/bootstrap_status",
        params: {}
      }),
      "utf8"
    );

    const finish = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      if (!socket.destroyed) {
        socket.destroy();
      }
      resolve(result);
    };

    const timer = setTimeout(() => {
      finish({
        reachable: true,
        compatible: false,
        reason: "Timed out waiting for bootstrap status",
        serverInfo: null,
        protocolVersion: null,
        bootstrapVersion: null
      });
    }, timeoutMs);

    socket.on("data", (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      while (true) {
        const headerEnd = buffer.indexOf("\r\n\r\n");
        if (headerEnd === -1) {
          return;
        }

        const header = buffer.subarray(0, headerEnd).toString("utf8");
        const contentLengthLine = header
          .split(/\r?\n/u)
          .find((line) => line.toLowerCase().startsWith("content-length:"));
        if (!contentLengthLine) {
          finish({
            reachable: true,
            compatible: false,
            reason: `Invalid bootstrap response header: ${header}`,
            serverInfo: null,
            protocolVersion: null,
            bootstrapVersion: null
          });
          return;
        }

        const length = Number(contentLengthLine.split(":")[1].trim());
        if (!Number.isInteger(length) || length < 0) {
          finish({
            reachable: true,
            compatible: false,
            reason: `Invalid bootstrap response length: ${contentLengthLine}`,
            serverInfo: null,
            protocolVersion: null,
            bootstrapVersion: null
          });
          return;
        }

        const bodyStart = headerEnd + 4;
        if (buffer.length < bodyStart + length) {
          return;
        }

        const payload = buffer.subarray(bodyStart, bodyStart + length);
        buffer = buffer.subarray(bodyStart + length);

        let message;
        try {
          message = JSON.parse(payload.toString("utf8"));
        } catch (error) {
          finish({
            reachable: true,
            compatible: false,
            reason: error instanceof Error ? error.message : String(error),
            serverInfo: null,
            protocolVersion: null,
            bootstrapVersion: null
          });
          return;
        }

        if (!message || typeof message !== "object") {
          finish({
            reachable: true,
            compatible: false,
            reason: "Bootstrap status returned a non-object payload",
            serverInfo: null,
            protocolVersion: null,
            bootstrapVersion: null
          });
          return;
        }

        if (message.error) {
          finish({
            reachable: true,
            compatible: false,
            reason: message.error.message ?? "Bootstrap status returned an error",
            serverInfo: null,
            protocolVersion: null,
            bootstrapVersion: null
          });
          return;
        }

        const result = message.result ?? null;
        const serverInfo =
          result && typeof result === "object" && !Array.isArray(result) ? result.serverInfo ?? null : null;
        const protocolVersion =
          result && typeof result === "object" && !Array.isArray(result) ? result.protocolVersion ?? null : null;
        const bootstrapVersion =
          result && typeof result === "object" && !Array.isArray(result) ? result.bootstrapVersion ?? null : null;
        const compatible =
          serverInfo &&
          serverInfo.name === SPIKI_SERVER_NAME &&
          serverInfo.version === SPIKI_SERVER_VERSION &&
          protocolVersion === SPIKI_PROTOCOL_VERSION &&
          bootstrapVersion === SPIKI_BOOTSTRAP_VERSION;

        finish({
          reachable: true,
          compatible,
          reason: compatible ? null : "Daemon bootstrap metadata does not match the current launcher",
          serverInfo,
          protocolVersion,
          bootstrapVersion
        });
        return;
      }
    });

    socket.once("close", () => {
      finish({
        reachable: true,
        compatible: false,
        reason: "Daemon closed the bootstrap probe connection early",
        serverInfo: null,
        protocolVersion: null,
        bootstrapVersion: null
      });
    });

    socket.once("error", (error) => {
      finish({
        reachable: true,
        compatible: false,
        reason: error instanceof Error ? error.message : String(error),
        serverInfo: null,
        protocolVersion: null,
        bootstrapVersion: null
      });
    });

    socket.write(`Content-Length: ${requestPayload.length}\r\n\r\n`);
    socket.write(requestPayload);
  });
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
  let lastStatus = null;
  while (Date.now() - startedAt < timeoutMs) {
    lastStatus = await probeDaemonStatus(socketPath, 250);
    if (lastStatus.compatible) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  if (lastStatus?.reachable && !lastStatus.compatible) {
    throw new Error(`Timed out waiting for a compatible spiki daemon: ${lastStatus.reason}`);
  }

  throw new Error("Timed out waiting for spiki daemon readiness");
}

async function waitForProcessExit(pid, timeoutMs = 3000) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if (!isProcessAlive(pid)) {
      return true;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  return !isProcessAlive(pid);
}

export async function ensureDaemonRunning() {
  const projectRoot = getProjectRoot();
  const runtimeDir = getRuntimeDir();
  const socketPath = getSocketPath(runtimeDir);
  const daemonBin = resolveDaemonBinary(projectRoot);

  await ensureRuntimeDir(runtimeDir);

  const initialProbe = await probeDaemonStatus(socketPath, 250);
  if (initialProbe.compatible) {
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

    let probe = await probeDaemonStatus(socketPath, 250);
    if (probe.compatible) {
      return { projectRoot, runtimeDir, socketPath, daemonBin };
    }
    if (probe.reachable) {
      const incompatiblePid = await readPid(runtimeDir);
      if (!incompatiblePid || !isProcessAlive(incompatiblePid)) {
        throw new Error(`Found an incompatible spiki daemon at ${socketPath} but could not resolve a live pid`);
      }

      process.kill(incompatiblePid, "SIGTERM");
      if (!(await waitForProcessExit(incompatiblePid, 3000))) {
        throw new Error(`Timed out stopping incompatible spiki daemon pid ${incompatiblePid}`);
      }
      await cleanupStaleRuntime(runtimeDir, socketPath);
      probe = await probeDaemonStatus(socketPath, 250);
      if (probe.compatible) {
        return { projectRoot, runtimeDir, socketPath, daemonBin };
      }
      if (probe.reachable) {
        throw new Error(`Incompatible spiki daemon remained reachable at ${socketPath} after restart`);
      }
    }

    await cleanupStaleRuntime(runtimeDir, socketPath);
    probe = await probeDaemonStatus(socketPath, 250);
    if (probe.compatible) {
      return { projectRoot, runtimeDir, socketPath, daemonBin };
    }

    const livePid = await readPid(runtimeDir);
    if (livePid && isProcessAlive(livePid)) {
      const liveDaemonDeadline = Date.now() + 2000;
      while (Date.now() < liveDaemonDeadline) {
        probe = await probeDaemonStatus(socketPath, 250);
        if (probe.compatible) {
          return { projectRoot, runtimeDir, socketPath, daemonBin };
        }

        if (!isProcessAlive(livePid)) {
          break;
        }

        await new Promise((resolve) => setTimeout(resolve, 100));
      }

      if (isProcessAlive(livePid)) {
        throw new Error(
          `Refusing to spawn a second spiki daemon while pid ${livePid} is alive but socket ${socketPath} is unreachable`
        );
      }

      await cleanupStaleRuntime(runtimeDir, socketPath);
      probe = await probeDaemonStatus(socketPath, 250);
      if (probe.compatible) {
        return { projectRoot, runtimeDir, socketPath, daemonBin };
      }
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

export async function daemonStatus() {
  const runtimeDir = getRuntimeDir();
  const socketPath = getSocketPath(runtimeDir);
  const daemonBin = resolveDaemonBinary(getProjectRoot());
  const pid = await readPid(runtimeDir);
  const probe = await probeDaemonStatus(socketPath, 250);

  return {
    runtimeDir,
    socketPath,
    daemonBin,
    pid,
    reachable: probe.reachable,
    compatible: probe.compatible,
    serverInfo: probe.serverInfo,
    protocolVersion: probe.protocolVersion,
    bootstrapVersion: probe.bootstrapVersion
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
  let stopped = await waitForProcessExit(pid, 3000);

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
