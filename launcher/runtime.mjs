import { daemonStatus } from "./daemon-bootstrap.mjs";

export { ensureDaemonRunning, daemonStatus, stopDaemon } from "./daemon-bootstrap.mjs";
export { bridgeStdio } from "./mcp-bridge.mjs";

export async function runDoctor() {
  const status = await daemonStatus();
  const lines = [
    `runtimeDir: ${status.runtimeDir}`,
    `socketPath: ${status.socketPath}`,
    `daemonBin: ${status.daemonBin}`,
    `pid: ${status.pid ?? "missing"}`,
    `reachable: ${status.reachable}`,
    `compatible: ${status.compatible}`
  ];

  process.stdout.write(`${lines.join("\n")}\n`);
}
