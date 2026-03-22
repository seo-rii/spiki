import { spawn } from "node:child_process";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

export async function createTestEnvironment(options = {}) {
  const prefix = options.prefix ?? "spiki-test-";
  const files = options.files ?? {};
  const instructions = options.instructions ?? null;
  const tempRoot = await mkdtemp(path.join(os.tmpdir(), prefix));
  const runtimeDir = path.join(tempRoot, "runtime");
  const workspaceDir = path.join(tempRoot, "workspace");

  await mkdir(runtimeDir, { recursive: true });
  await mkdir(workspaceDir, { recursive: true });

  if (instructions) {
    await writeFile(path.join(workspaceDir, "AGENTS.md"), `${instructions.trim()}\n`);
  }

  for (const [relativePath, contents] of Object.entries(files)) {
    const absolutePath = path.join(workspaceDir, relativePath);
    await mkdir(path.dirname(absolutePath), { recursive: true });
    await writeFile(absolutePath, contents);
  }

  return {
    tempRoot,
    runtimeDir,
    workspaceDir,
    rootUri: pathToFileURL(workspaceDir).toString(),
    env: {
      ...process.env,
      AGENT_EDITOR_RUNTIME_DIR: runtimeDir
    },
    async cleanup() {
      await rm(tempRoot, { recursive: true, force: true });
    }
  };
}

export async function runProcess(command, args, options = {}) {
  const cwd = options.cwd ?? projectRoot;
  const env = options.env ?? process.env;
  const stdinText = options.stdinText;
  const timeoutMs = options.timeoutMs ?? 5000;

  return await new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env,
      stdio: ["pipe", "pipe", "pipe"]
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`Timed out waiting for ${command}`));
    }, timeoutMs);

    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString("utf8");
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString("utf8");
    });
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal, stdout, stderr });
    });

    if (stdinText !== undefined) {
      child.stdin.end(stdinText);
      return;
    }

    child.stdin.end();
  });
}
