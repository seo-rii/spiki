#!/usr/bin/env node

import { main } from "../launcher/cli.mjs";

main().catch((error) => {
  console.error(error instanceof Error ? error.stack ?? error.message : String(error));
  process.exitCode = 1;
});
