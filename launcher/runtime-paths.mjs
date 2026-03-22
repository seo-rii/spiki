import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function getProjectRoot() {
  return path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
}

export function getRuntimeDir() {
  return (
    process.env.AGENT_EDITOR_RUNTIME_DIR ??
    process.env.SPIKI_RUNTIME_DIR ??
    path.join(os.tmpdir(), `spiki-${process.getuid?.() ?? os.userInfo().username}`)
  );
}

export function getSocketPath(runtimeDir) {
  if (process.platform === "win32") {
    const user = (process.env.USERNAME ?? os.userInfo().username).replaceAll(/[^a-zA-Z0-9_-]/g, "_");
    return `\\\\.\\pipe\\spiki-${user}`;
  }

  return path.join(runtimeDir, "daemon.sock");
}

export function resolveDaemonBinary(projectRoot) {
  if (process.env.SPIKI_DAEMON_BIN) {
    return process.env.SPIKI_DAEMON_BIN;
  }

  const binaryName = process.platform === "win32" ? "spiki-daemon.exe" : "spiki-daemon";
  return path.join(projectRoot, "target", "debug", binaryName);
}
