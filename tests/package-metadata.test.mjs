import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import { projectRoot, runProcess } from "./lib/test-env.mjs";

test("package metadata is publish-ready", { timeout: 30000 }, async () => {
  const packageJson = JSON.parse(await readFile(path.join(projectRoot, "package.json"), "utf8"));

  assert.notEqual(packageJson.private, true);
  assert.equal(packageJson.name, "@seo-rii/spiki");
  assert.equal(packageJson.bin.spiki, "./bin/spiki.js");
  assert.ok(packageJson.description);
  assert.ok(packageJson.license);
  assert.equal(packageJson.repository?.type, "git");
  assert.match(packageJson.repository?.url ?? "", /seo-rii\/spiki/);
  assert.match(packageJson.homepage ?? "", /seo-rii\/spiki/);
  assert.match(packageJson.bugs?.url ?? "", /seo-rii\/spiki/);
  assert.ok(Array.isArray(packageJson.files));
  assert.ok(packageJson.files.includes("bin/"));
  assert.ok(packageJson.files.includes("launcher/"));
  assert.ok(packageJson.files.includes("crates/"));
  assert.ok(packageJson.files.includes("scripts/"));
  assert.ok(packageJson.files.includes("README.md"));
  assert.ok(packageJson.files.includes("SPEC.md"));

  const packResult = await runProcess("npm", ["pack", "--json", "--dry-run"], {
    cwd: projectRoot,
    timeoutMs: 30000
  });
  assert.equal(packResult.code, 0, packResult.stderr);

  const packOutput = JSON.parse(packResult.stdout);
  const packedFiles = new Set(packOutput[0].files.map((file) => file.path));
  assert.ok(packedFiles.has("bin/spiki.js"));
  assert.ok(packedFiles.has("launcher/runtime.mjs"));
  assert.ok(packedFiles.has("scripts/build-daemon.mjs"));
  assert.ok(packedFiles.has("crates/spiki-daemon/src/main.rs"));
  assert.ok(packedFiles.has("README.md"));
  assert.ok(packedFiles.has("SPEC.md"));
});
