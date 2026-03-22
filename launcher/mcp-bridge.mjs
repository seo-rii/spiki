import path from "node:path";
import { pathToFileURL } from "node:url";

import { connectSocket, ensureDaemonRunning } from "./daemon-bootstrap.mjs";

export async function bridgeStdio() {
  const { socketPath } = await ensureDaemonRunning();
  const socket = await connectSocket(socketPath, 1000);
  let finished = false;
  let clientMode = null;
  let stdinBuffer = Buffer.alloc(0);
  let socketBuffer = Buffer.alloc(0);
  const allowCwdRootFallback = process.env.SPIKI_ALLOW_CWD_ROOT_FALLBACK === "1";

  process.stdin.resume();
  const finish = (reject, error) => {
    if (finished) {
      return;
    }
    finished = true;
    if (error) {
      reject(error);
      return;
    }
    if (!socket.destroyed) {
      socket.destroy();
    }
  };

  process.stdin.once("end", () => {
    socket.end();
    setTimeout(() => {
      if (!finished) {
        socket.destroy();
      }
    }, 100).unref();
  });
  process.stdin.once("close", () => {
    socket.end();
  });
  process.stdin.on("data", (chunk) => {
    stdinBuffer = Buffer.concat([stdinBuffer, chunk]);

    while (stdinBuffer.length > 0) {
      if (!clientMode) {
        const preview = stdinBuffer.subarray(0, Math.min(stdinBuffer.length, 64)).toString("utf8");
        if (preview.trimStart().startsWith("{")) {
          clientMode = "jsonl";
        } else if (preview.toLowerCase().startsWith("content-length:")) {
          clientMode = "content-length";
        } else {
          return;
        }
      }

      if (clientMode === "content-length") {
        const headerEnd = stdinBuffer.indexOf("\r\n\r\n");
        if (headerEnd === -1) {
          return;
        }

        const header = stdinBuffer.subarray(0, headerEnd).toString("utf8");
        const contentLengthLine = header
          .split(/\r?\n/u)
          .find((line) => line.toLowerCase().startsWith("content-length:"));
        if (!contentLengthLine) {
          throw new Error(`Missing Content-Length header: ${header}`);
        }

        const length = Number(contentLengthLine.split(":")[1].trim());
        const bodyStart = headerEnd + 4;
        if (stdinBuffer.length < bodyStart + length) {
          return;
        }

        const payload = stdinBuffer.subarray(bodyStart, bodyStart + length);
        stdinBuffer = stdinBuffer.subarray(bodyStart + length);
        const message = JSON.parse(payload.toString("utf8"));

        if (
          message.method === "initialize" &&
          !message.params?.roots &&
          !message.params?.capabilities?.roots
        ) {
          if (!allowCwdRootFallback) {
            const response = {
              jsonrpc: "2.0",
              id: message.id ?? null,
              error: {
                code: -32602,
                message:
                  "Client must provide initialize.params.roots or set SPIKI_ALLOW_CWD_ROOT_FALLBACK=1"
              }
            };
            const responsePayload = Buffer.from(JSON.stringify(response), "utf8");
            process.stdout.write(`Content-Length: ${responsePayload.length}\r\n\r\n`);
            process.stdout.write(responsePayload);
            continue;
          }

          const params = message.params ?? {};
          message.params = {
            ...params,
            roots: [{ uri: pathToFileURL(process.cwd()).toString(), name: path.basename(process.cwd()) || "workspace" }]
          };
        }

        const forwardPayload = Buffer.from(JSON.stringify(message), "utf8");
        socket.write(`Content-Length: ${forwardPayload.length}\r\n\r\n`);
        socket.write(forwardPayload);
        continue;
      }

      const newlineIndex = stdinBuffer.indexOf("\n");
      if (newlineIndex === -1) {
        return;
      }

      const line = stdinBuffer.subarray(0, newlineIndex).toString("utf8").trim();
      stdinBuffer = stdinBuffer.subarray(newlineIndex + 1);
      if (line.length === 0) {
        continue;
      }

      const message = JSON.parse(line);
      if (
        message.method === "initialize" &&
        !message.params?.roots &&
        !message.params?.capabilities?.roots
      ) {
        if (!allowCwdRootFallback) {
          process.stdout.write(
            `${JSON.stringify({
              jsonrpc: "2.0",
              id: message.id ?? null,
              error: {
                code: -32602,
                message:
                  "Client must provide initialize.params.roots or set SPIKI_ALLOW_CWD_ROOT_FALLBACK=1"
              }
            })}\n`
          );
          continue;
        }

        const params = message.params ?? {};
        message.params = {
          ...params,
          roots: [{ uri: pathToFileURL(process.cwd()).toString(), name: path.basename(process.cwd()) || "workspace" }]
        };
      }

      const outgoingPayload = Buffer.from(JSON.stringify(message), "utf8");
      socket.write(`Content-Length: ${outgoingPayload.length}\r\n\r\n`);
      socket.write(outgoingPayload);
    }
  });
  socket.on("data", (chunk) => {
    socketBuffer = Buffer.concat([socketBuffer, chunk]);

    while (true) {
      const headerEnd = socketBuffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) {
        return;
      }

      const header = socketBuffer.subarray(0, headerEnd).toString("utf8");
      const contentLengthLine = header
        .split(/\r?\n/u)
        .find((line) => line.toLowerCase().startsWith("content-length:"));
      if (!contentLengthLine) {
        throw new Error(`Missing Content-Length header from daemon: ${header}`);
      }

      const length = Number(contentLengthLine.split(":")[1].trim());
      const bodyStart = headerEnd + 4;
      if (socketBuffer.length < bodyStart + length) {
        return;
      }

      const payload = socketBuffer.subarray(bodyStart, bodyStart + length);
      socketBuffer = socketBuffer.subarray(bodyStart + length);
      const message = JSON.parse(payload.toString("utf8"));

      if (clientMode === "jsonl") {
        process.stdout.write(`${JSON.stringify(message)}\n`);
        continue;
      }

      process.stdout.write(`Content-Length: ${payload.length}\r\n\r\n`);
      process.stdout.write(payload);
    }
  });

  const shutdown = () => {
    socket.end();
  };

  process.once("SIGINT", shutdown);
  process.once("SIGTERM", shutdown);

  await new Promise((resolve, reject) => {
    socket.once("close", () => {
      if (finished) {
        resolve();
        return;
      }
      finished = true;
      resolve();
    });
    socket.once("error", (error) => finish(reject, error));
  });
}
