import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  getBundledDaemonBinary,
  getDaemonBinaryName,
  getRuntimeDir,
  getNativeBundleId,
  resolveDaemonBinary
} from "../launcher/runtime-paths.mjs";

async function withTemporaryEnv(overrides, callback) {
  const previous = new Map();
  for (const [key, value] of Object.entries(overrides)) {
    previous.set(key, process.env[key]);
    if (value === undefined) {
      delete process.env[key];
      continue;
    }
    process.env[key] = value;
  }

  try {
    await callback();
  } finally {
    for (const [key, value] of previous.entries()) {
      if (value === undefined) {
        delete process.env[key];
        continue;
      }
      process.env[key] = value;
    }
  }
}

test("getRuntimeDir uses a user-scoped default path", async () => {
  await withTemporaryEnv(
    {
      AGENT_EDITOR_RUNTIME_DIR: undefined,
      SPIKI_RUNTIME_DIR: undefined,
      XDG_RUNTIME_DIR: undefined,
      XDG_CACHE_HOME: undefined,
      LOCALAPPDATA: undefined
    },
    async () => {
      if (process.platform === "win32") {
        assert.equal(getRuntimeDir(), path.join(os.homedir(), "AppData", "Local", "spiki"));
        return;
      }

      if (process.platform === "darwin") {
        assert.equal(getRuntimeDir(), path.join(os.homedir(), "Library", "Caches", "spiki"));
        return;
      }

      assert.equal(getRuntimeDir(), path.join(os.homedir(), ".cache", "spiki"));
    }
  );
});

test("getRuntimeDir honors XDG_RUNTIME_DIR on Unix-like hosts", async () => {
  if (process.platform === "win32" || process.platform === "darwin") {
    return;
  }

  await withTemporaryEnv(
    {
      AGENT_EDITOR_RUNTIME_DIR: undefined,
      SPIKI_RUNTIME_DIR: undefined,
      XDG_RUNTIME_DIR: "/tmp/spiki-runtime-test",
      XDG_CACHE_HOME: undefined
    },
    async () => {
      assert.equal(getRuntimeDir(), "/tmp/spiki-runtime-test/spiki");
    }
  );
});

test("resolveDaemonBinary prefers a packaged native daemon bundle", async (t) => {
  const tempRoot = await mkdtemp(path.join(os.tmpdir(), "spiki-runtime-paths-"));
  t.after(async () => {
    await rm(tempRoot, { recursive: true, force: true });
  });

  const bundledBinary = getBundledDaemonBinary(tempRoot);
  const debugBinary = path.join(tempRoot, "target", "debug", getDaemonBinaryName());
  await mkdir(path.dirname(bundledBinary), { recursive: true });
  await mkdir(path.dirname(debugBinary), { recursive: true });
  await writeFile(debugBinary, "debug");
  assert.equal(resolveDaemonBinary(tempRoot), debugBinary);

  await writeFile(bundledBinary, "bundled");
  assert.equal(
    resolveDaemonBinary(tempRoot),
    path.join(tempRoot, "bin", "native", getNativeBundleId(), getDaemonBinaryName())
  );
});
