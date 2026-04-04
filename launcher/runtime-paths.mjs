import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function getProjectRoot() {
  return path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
}

export function getRuntimeDir() {
  if (process.env.AGENT_EDITOR_RUNTIME_DIR) {
    return process.env.AGENT_EDITOR_RUNTIME_DIR;
  }

  if (process.env.SPIKI_RUNTIME_DIR) {
    return process.env.SPIKI_RUNTIME_DIR;
  }

  if (process.platform === "win32") {
    return path.join(process.env.LOCALAPPDATA ?? path.join(os.homedir(), "AppData", "Local"), "spiki");
  }

  if (process.platform === "darwin") {
    return path.join(os.homedir(), "Library", "Caches", "spiki");
  }

  if (process.env.XDG_RUNTIME_DIR) {
    return path.join(process.env.XDG_RUNTIME_DIR, "spiki");
  }

  return path.join(process.env.XDG_CACHE_HOME ?? path.join(os.homedir(), ".cache"), "spiki");
}

export function getSocketPath(runtimeDir) {
  if (process.platform === "win32") {
    const user = (process.env.USERNAME ?? os.userInfo().username).replaceAll(/[^a-zA-Z0-9_-]/g, "_");
    return `\\\\.\\pipe\\spiki-${user}`;
  }

  return path.join(runtimeDir, "daemon.sock");
}

export function getDaemonBinaryName() {
  return process.platform === "win32" ? "spiki-daemon.exe" : "spiki-daemon";
}

export function getNativeBundleId() {
  return `${process.platform}-${process.arch}`;
}

export function getBundledDaemonBinary(projectRoot) {
  return path.join(projectRoot, "bin", "native", getNativeBundleId(), getDaemonBinaryName());
}

export function resolveDaemonBinary(projectRoot) {
  if (process.env.SPIKI_DAEMON_BIN) {
    return process.env.SPIKI_DAEMON_BIN;
  }

  const bundledBinary = getBundledDaemonBinary(projectRoot);
  if (fs.existsSync(bundledBinary)) {
    return bundledBinary;
  }

  return path.join(projectRoot, "target", "debug", getDaemonBinaryName());
}
