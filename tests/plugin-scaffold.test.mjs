import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { projectRoot, runProcess } from "./lib/test-env.mjs";

async function scaffoldPlugin(client, options = {}) {
  const outputDir = await mkdtemp(path.join(os.tmpdir(), `spiki-${client}-plugin-`));
  const args = ["./bin/spiki.js", "plugin", "scaffold", client, outputDir];
  if (options.allowCwdRootFallback) {
    args.push("--allow-cwd-root-fallback");
  }

  const result = await runProcess(process.execPath, args, {
    cwd: projectRoot,
    timeoutMs: 60000
  });

  return { outputDir, result };
}

async function readJson(relativePath) {
  return JSON.parse(await readFile(relativePath, "utf8"));
}

function assertLauncherReference(serverConfig) {
  const command = typeof serverConfig.command === "string" ? serverConfig.command : "";
  const args = Array.isArray(serverConfig.args) ? serverConfig.args : [];
  const commandEndsWithLauncher = command.endsWith("/bin/spiki.js") || command.endsWith("\\bin\\spiki.js");
  const argsContainLauncher = args.some(
    (value) => typeof value === "string" && (value.endsWith("/bin/spiki.js") || value.endsWith("\\bin\\spiki.js"))
  );

  assert.ok(
    commandEndsWithLauncher || argsContainLauncher,
    `expected launcher reference in ${JSON.stringify(serverConfig)}`
  );
}

function extractServerConfigs(mcpJson) {
  if (mcpJson && typeof mcpJson === "object" && !Array.isArray(mcpJson)) {
    if (mcpJson.mcpServers && typeof mcpJson.mcpServers === "object" && !Array.isArray(mcpJson.mcpServers)) {
      return Object.values(mcpJson.mcpServers);
    }
    return Object.values(mcpJson);
  }
  return [];
}

test("plugin scaffold writes a Codex bundle", { timeout: 60000 }, async (t) => {
  const { outputDir, result } = await scaffoldPlugin("codex", { allowCwdRootFallback: true });
  t.after(async () => {
    await rm(outputDir, { recursive: true, force: true });
  });

  assert.equal(result.code, 0, `stdout:\n${result.stdout}\nstderr:\n${result.stderr}`);

  const manifestPath = path.join(outputDir, ".codex-plugin", "plugin.json");
  const mcpConfigPath = path.join(outputDir, ".mcp.json");
  const manifest = await readJson(manifestPath);
  const mcpJson = await readJson(mcpConfigPath);
  const serverConfigs = extractServerConfigs(mcpJson);

  assert.equal(manifest.name, "spiki");
  assert.equal(manifest.mcpServers, "./.mcp.json");
  assert.equal(manifest.interface?.displayName, "spiki");
  assert.ok(manifest.interface?.shortDescription);
  assert.ok(manifest.interface?.longDescription);
  assert.ok(manifest.interface?.category);
  assert.ok(manifest.repository);
  assert.ok(manifest.homepage);
  assert.ok(manifest.description);

  assert.ok(serverConfigs.length >= 1, `expected at least one MCP server config in ${mcpConfigPath}`);
  assertLauncherReference(serverConfigs[0]);
  assert.match(JSON.stringify(mcpJson), /SPIKI_ALLOW_CWD_ROOT_FALLBACK/);
});

test("plugin scaffold writes a Claude bundle", { timeout: 60000 }, async (t) => {
  const { outputDir, result } = await scaffoldPlugin("claude");
  t.after(async () => {
    await rm(outputDir, { recursive: true, force: true });
  });

  assert.equal(result.code, 0, `stdout:\n${result.stdout}\nstderr:\n${result.stderr}`);

  const manifestPath = path.join(outputDir, "plugin.json");
  const mcpConfigPath = path.join(outputDir, ".mcp.json");
  const manifest = await readJson(manifestPath);
  const mcpJson = await readJson(mcpConfigPath);
  const serverConfigs = extractServerConfigs(mcpJson);
  const inlineServerConfigs = extractServerConfigs({ mcpServers: manifest.mcpServers });

  assert.equal(manifest.name, "spiki");
  assert.ok(manifest.repository);
  assert.ok(manifest.homepage);
  assert.ok(manifest.description);
  assert.equal(typeof manifest.mcpServers, "object");

  assert.ok(serverConfigs.length >= 1, `expected at least one MCP server config in ${mcpConfigPath}`);
  assertLauncherReference(serverConfigs[0]);
  assert.ok(inlineServerConfigs.length >= 1, `expected inline MCP server config in ${manifestPath}`);
  assertLauncherReference(inlineServerConfigs[0]);
  assert.doesNotMatch(JSON.stringify(mcpJson), /SPIKI_ALLOW_CWD_ROOT_FALLBACK/);
});
