import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import net from "node:net";
import path from "node:path";

import { getProjectRoot, getRuntimeDir, getSocketPath, resolveDaemonBinary } from "./runtime-paths.mjs";

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
