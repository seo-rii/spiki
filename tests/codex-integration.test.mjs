import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import { createTestEnvironment, projectRoot, runProcess } from "./lib/test-env.mjs";

test("Codex exec can call spiki MCP tools", { timeout: 180000 }, async (t) => {
  const token = `codex_integration_token_${Date.now().toString(36)}`;
  const context = await createTestEnvironment({
    prefix: "spiki-codex-",
    instructions: [
      "Always use the spiki MCP server for workspace inspection tasks in this repository.",
      "Prefer spiki tools over shell search or js_repl when checking project files."
    ].join("\n"),
    files: {
      "index.ts": `const ${token} = 1;\nconsole.log(${token});\n`
    }
  });
  t.after(async () => {
    await runProcess(process.execPath, ["./bin/spiki.js", "daemon", "stop"], {
      cwd: projectRoot,
      env: context.env,
      timeoutMs: 5000
    }).catch(() => {});
    await context.cleanup();
  });

  const schemaPath = path.join(context.tempRoot, "codex-schema.json");
  const outputPath = path.join(context.tempRoot, "codex-output.json");
  await writeFile(
    schemaPath,
    JSON.stringify(
      {
        $schema: "https://json-schema.org/draft/2020-12/schema",
        type: "object",
        properties: {
          matches: {
            type: "integer"
          }
        },
        required: ["matches"],
        additionalProperties: false
      },
      null,
      2
    )
  );

  const prompt = [
    `Use the spiki MCP tool ae.workspace.search_text to count the exact number of occurrences of ${token} in the active workspace.`,
    "Return JSON with the integer field matches only."
  ].join(" ");
  const env = {
    ...context.env,
    SPIKI_ALLOW_CWD_ROOT_FALLBACK: "1"
  };
  const codexArgs = [
    "exec",
    "--skip-git-repo-check",
    "--ephemeral",
    "--json",
    "--dangerously-bypass-approvals-and-sandbox",
    "-c",
    'model_reasoning_effort="low"',
    "-c",
    "suppress_unstable_features_warning=true",
    "-c",
    'mcp_servers.spiki.command="node"',
    "-c",
    `mcp_servers.spiki.args=${JSON.stringify([path.join(projectRoot, "bin", "spiki.js")])}`,
    "-c",
    'mcp_servers.spiki.env={ SPIKI_ALLOW_CWD_ROOT_FALLBACK = "1" }',
    "-c",
    `mcp_servers.spiki.cwd=${JSON.stringify(context.workspaceDir)}`,
    "--output-schema",
    schemaPath,
    "-o",
    outputPath,
    "-C",
    context.workspaceDir,
    prompt
  ];

  let codexResult;
  try {
    codexResult = await runProcess("codex", codexArgs, {
      cwd: projectRoot,
      env,
      timeoutMs: 180000
    });
  } catch (error) {
    if (error && error.code !== "ENOENT") {
      throw error;
    }

    codexResult = await runProcess("npx", ["-y", "@openai/codex", ...codexArgs], {
      cwd: projectRoot,
      env,
      timeoutMs: 180000
    });
  }

  assert.equal(
    codexResult.code,
    0,
    `codex exec failed\nstdout:\n${codexResult.stdout}\nstderr:\n${codexResult.stderr}`
  );

  const events = codexResult.stdout
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line));
  const spikiToolCalls = events.filter(
    (event) =>
      event.type === "item.completed" &&
      event.item?.type === "mcp_tool_call" &&
      event.item.server === "spiki"
  );

  assert.ok(
    spikiToolCalls.some(
      (event) => event.item.tool === "ae.workspace.search_text" && event.item.status === "completed" && !event.item.error
    ),
    `expected a completed spiki ae.workspace.search_text call\nstdout:\n${codexResult.stdout}\nstderr:\n${codexResult.stderr}`
  );

  const output = JSON.parse(await readFile(outputPath, "utf8"));
  assert.deepEqual(output, { matches: 2 });
});
