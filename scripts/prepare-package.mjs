import { spawnSync } from "node:child_process";
import { chmod, copyFile, mkdir } from "node:fs/promises";
import path from "node:path";

import { getBundledDaemonBinary, getDaemonBinaryName, getProjectRoot } from "../launcher/runtime-paths.mjs";

const projectRoot = getProjectRoot();
const bundledBinary = getBundledDaemonBinary(projectRoot);
const targetBinary = path.join(projectRoot, "target", "debug", getDaemonBinaryName());

const build = spawnSync(process.execPath, ["./scripts/build-daemon.mjs"], {
  cwd: projectRoot,
  stdio: "inherit"
});

if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

await mkdir(path.dirname(bundledBinary), { recursive: true });
await copyFile(targetBinary, bundledBinary);
if (process.platform !== "win32") {
  await chmod(bundledBinary, 0o755);
}
