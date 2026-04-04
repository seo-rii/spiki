import path from "node:path";
import { rm } from "node:fs/promises";

import { getProjectRoot } from "../launcher/runtime-paths.mjs";

await rm(path.join(getProjectRoot(), "bin", "native"), { recursive: true, force: true });
