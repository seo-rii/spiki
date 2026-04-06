import fs from "node:fs/promises";
import path from "node:path";

import { bridgeStdio, daemonStatus, runDoctor, stopDaemon } from "./runtime.mjs";
import { getProjectRoot } from "./runtime-paths.mjs";

export async function main() {
  const [command, subcommand, ...rest] = process.argv.slice(2);

  if (command === "doctor") {
    await runDoctor();
    return;
  }

  if (command === "daemon" && subcommand === "status") {
    process.stdout.write(`${JSON.stringify(await daemonStatus(), null, 2)}\n`);
    return;
  }

  if (command === "daemon" && subcommand === "stop") {
    process.stdout.write(`${JSON.stringify(await stopDaemon(), null, 2)}\n`);
    return;
  }

  if (command === "plugin" && subcommand === "scaffold") {
    const [client, outputDirArg, ...flags] = rest;
    if (client !== "codex" && client !== "claude") {
      throw new Error("Usage: spiki plugin scaffold <codex|claude> <outputDir> [--allow-cwd-root-fallback]");
    }
    if (!outputDirArg) {
      throw new Error("Usage: spiki plugin scaffold <codex|claude> <outputDir> [--allow-cwd-root-fallback]");
    }
    if (flags.some((flag) => flag !== "--allow-cwd-root-fallback")) {
      throw new Error(`Unknown plugin scaffold option: ${flags.find((flag) => flag !== "--allow-cwd-root-fallback")}`);
    }

    const allowCwdRootFallback = flags.includes("--allow-cwd-root-fallback");
    const projectRoot = getProjectRoot();
    const launcherPath = path.join(projectRoot, "bin", "spiki.js");
    const packageJson = JSON.parse(await fs.readFile(path.join(projectRoot, "package.json"), "utf8"));
    const outputDir = path.resolve(outputDirArg);
    const mcpConfig = {
      mcpServers: {
        spiki: {
          command: "node",
          args: [launcherPath]
        }
      }
    };

    if (allowCwdRootFallback) {
      mcpConfig.mcpServers.spiki.env = {
        SPIKI_ALLOW_CWD_ROOT_FALLBACK: "1"
      };
    }

    await fs.mkdir(outputDir, { recursive: true });
    await fs.writeFile(path.join(outputDir, ".mcp.json"), `${JSON.stringify(mcpConfig, null, 2)}\n`);

    const repository =
      packageJson.repository && typeof packageJson.repository === "object"
        ? packageJson.repository.url
        : packageJson.repository;
    const codexManifest = {
      name: packageJson.name,
      version: packageJson.version,
      description: packageJson.description,
      author:
        typeof packageJson.author === "object"
          ? packageJson.author
          : {
              name: packageJson.author ?? "seo-rii"
            },
      homepage: packageJson.homepage,
      repository,
      license: packageJson.license,
      keywords: packageJson.keywords,
      mcpServers: "./.mcp.json",
      interface: {
        displayName: "spiki",
        shortDescription: "Workspace-native MCP for editor-style code operations.",
        longDescription:
          "spiki exposes stable workspace inspection, search, plan, and semantic tools through an MCP bridge that can be bundled into Codex and Claude plugins.",
        developerName: "seo-rii",
        category: "Developer Tools",
        capabilities: ["Interactive", "Write", "Workspace"],
        websiteURL: packageJson.homepage
      }
    };

    if (client === "codex") {
      await fs.mkdir(path.join(outputDir, ".codex-plugin"), { recursive: true });
      await fs.writeFile(
        path.join(outputDir, ".codex-plugin", "plugin.json"),
        `${JSON.stringify(codexManifest, null, 2)}\n`
      );
      process.stdout.write(
        `${JSON.stringify(
          {
            client,
            outputDir,
            files: [".mcp.json", ".codex-plugin/plugin.json"]
          },
          null,
          2
        )}\n`
      );
      return;
    }

    await fs.writeFile(
      path.join(outputDir, "plugin.json"),
      `${JSON.stringify(
        {
          name: packageJson.name,
          version: packageJson.version,
          description: packageJson.description,
          author: packageJson.author,
          homepage: packageJson.homepage,
          repository,
          license: packageJson.license,
          mcpServers: mcpConfig.mcpServers
        },
        null,
        2
      )}\n`
    );
    process.stdout.write(
      `${JSON.stringify(
        {
          client,
          outputDir,
          files: [".mcp.json", "plugin.json"]
        },
        null,
        2
      )}\n`
    );
    return;
  }

  await bridgeStdio();
}
