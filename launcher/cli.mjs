import { bridgeStdio, daemonStatus, runDoctor, stopDaemon } from "./runtime.mjs";

export async function main() {
  const [command, subcommand] = process.argv.slice(2);

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

  await bridgeStdio();
}
