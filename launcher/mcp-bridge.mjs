import path from "node:path";
import { pathToFileURL } from "node:url";

import { connectSocket, ensureDaemonRunning } from "./daemon-bootstrap.mjs";

const MAX_BRIDGE_FRAME_BYTES = 1024 * 1024;

function ensureBufferedPayloadWithinLimit(bufferLength, source) {
  if (bufferLength > MAX_BRIDGE_FRAME_BYTES) {
    throw new Error(`${source} buffer exceeded ${MAX_BRIDGE_FRAME_BYTES} bytes`);
  }
}

function parseBoundedContentLength(header, source) {
  const contentLengthLine = header
    .split(/\r?\n/u)
    .find((line) => line.toLowerCase().startsWith("content-length:"));
  if (!contentLengthLine) {
    throw new Error(`Missing Content-Length header from ${source}: ${header}`);
  }

  const length = Number(contentLengthLine.split(":")[1].trim());
  if (!Number.isInteger(length) || length < 0) {
    throw new Error(`Invalid Content-Length header from ${source}: ${contentLengthLine}`);
  }
  if (length > MAX_BRIDGE_FRAME_BYTES) {
    throw new Error(`${source} frame exceeds ${MAX_BRIDGE_FRAME_BYTES} bytes`);
  }
  return length;
}

function buildInitializeErrorResponse(message, errorMessage) {
  return {
    jsonrpc: "2.0",
    id: message.id ?? null,
    error: {
      code: -32602,
      message: errorMessage
    }
  };
}

function writeClientResponse(response, clientMode) {
  const payload = Buffer.from(JSON.stringify(response), "utf8");
  if (clientMode === "content-length") {
    process.stdout.write(`Content-Length: ${payload.length}\r\n\r\n`);
    process.stdout.write(payload);
    return;
  }
  process.stdout.write(`${payload.toString("utf8")}\n`);
}

function normalizeInitializeMessage(message, allowCwdRootFallback) {
  if (message.method !== "initialize") {
    return { message, response: null };
  }
  if (Array.isArray(message.params?.roots) && message.params.roots.length === 0) {
    return {
      message,
      response: buildInitializeErrorResponse(message, "initialize.params.roots must not be empty")
    };
  }
  if (message.params?.roots != null || message.params?.capabilities?.roots) {
    return { message, response: null };
  }
  if (!allowCwdRootFallback) {
    return {
      message,
      response: buildInitializeErrorResponse(
        message,
        "Client must provide initialize.params.roots or set SPIKI_ALLOW_CWD_ROOT_FALLBACK=1"
      )
    };
  }

  const params = message.params ?? {};
  return {
    message: {
      ...message,
      params: {
        ...params,
        roots: [{ uri: pathToFileURL(process.cwd()).toString(), name: path.basename(process.cwd()) || "workspace" }]
      }
    },
    response: null
  };
}

export async function bridgeStdio() {
  const { socketPath } = await ensureDaemonRunning();
  const socket = await connectSocket(socketPath, 1000);
  let finished = false;
  let clientMode = null;
  let stdinBuffer = Buffer.alloc(0);
  let socketBuffer = Buffer.alloc(0);
  let rejectBridge = null;
  const allowCwdRootFallback = process.env.SPIKI_ALLOW_CWD_ROOT_FALLBACK === "1";

  process.stdin.resume();
  const finish = (error) => {
    if (finished) {
      return;
    }
    finished = true;
    if (error) {
      if (!socket.destroyed) {
        socket.destroy();
      }
      rejectBridge?.(error);
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
    try {
      stdinBuffer = Buffer.concat([stdinBuffer, chunk]);
      ensureBufferedPayloadWithinLimit(stdinBuffer.length, "client");

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
          const length = parseBoundedContentLength(header, "client");
          const bodyStart = headerEnd + 4;
          if (stdinBuffer.length < bodyStart + length) {
            return;
          }

          const payload = stdinBuffer.subarray(bodyStart, bodyStart + length);
          stdinBuffer = stdinBuffer.subarray(bodyStart + length);
          let message;
          try {
            message = JSON.parse(payload.toString("utf8"));
          } catch (error) {
            finish(
              new Error(
                `Invalid JSON from client: ${error instanceof Error ? error.message : String(error)}`
              )
            );
            return;
          }
          const normalized = normalizeInitializeMessage(message, allowCwdRootFallback);
          if (normalized.response) {
            writeClientResponse(normalized.response, clientMode);
            continue;
          }
          message = normalized.message;

          const forwardPayload = Buffer.from(JSON.stringify(message), "utf8");
          if (forwardPayload.length > MAX_BRIDGE_FRAME_BYTES) {
            finish(new Error(`client frame exceeds ${MAX_BRIDGE_FRAME_BYTES} bytes`));
            return;
          }
          socket.write(`Content-Length: ${forwardPayload.length}\r\n\r\n`);
          socket.write(forwardPayload);
          continue;
        }

        const newlineIndex = stdinBuffer.indexOf("\n");
        if (newlineIndex === -1) {
          ensureBufferedPayloadWithinLimit(stdinBuffer.length, "client");
          return;
        }

        const line = stdinBuffer.subarray(0, newlineIndex).toString("utf8").trim();
        stdinBuffer = stdinBuffer.subarray(newlineIndex + 1);
        if (line.length === 0) {
          continue;
        }

        let message;
        try {
          message = JSON.parse(line);
        } catch (error) {
          finish(
            new Error(
              `Invalid JSON from client: ${error instanceof Error ? error.message : String(error)}`
            )
          );
          return;
        }
        const normalized = normalizeInitializeMessage(message, allowCwdRootFallback);
        if (normalized.response) {
          writeClientResponse(normalized.response, clientMode);
          continue;
        }
        message = normalized.message;

        const outgoingPayload = Buffer.from(JSON.stringify(message), "utf8");
        if (outgoingPayload.length > MAX_BRIDGE_FRAME_BYTES) {
          finish(new Error(`client frame exceeds ${MAX_BRIDGE_FRAME_BYTES} bytes`));
          return;
        }
        socket.write(`Content-Length: ${outgoingPayload.length}\r\n\r\n`);
        socket.write(outgoingPayload);
      }
    } catch (error) {
      finish(error instanceof Error ? error : new Error(String(error)));
    }
  });
  socket.on("data", (chunk) => {
    try {
      socketBuffer = Buffer.concat([socketBuffer, chunk]);
      ensureBufferedPayloadWithinLimit(socketBuffer.length, "daemon");

      while (true) {
        const headerEnd = socketBuffer.indexOf("\r\n\r\n");
        if (headerEnd === -1) {
          return;
        }

        const header = socketBuffer.subarray(0, headerEnd).toString("utf8");
        const length = parseBoundedContentLength(header, "daemon");
        const bodyStart = headerEnd + 4;
        if (socketBuffer.length < bodyStart + length) {
          return;
        }

        const payload = socketBuffer.subarray(bodyStart, bodyStart + length);
        socketBuffer = socketBuffer.subarray(bodyStart + length);
        let message;
        try {
          message = JSON.parse(payload.toString("utf8"));
        } catch (error) {
          finish(
            new Error(
              `Invalid JSON from daemon: ${error instanceof Error ? error.message : String(error)}`
            )
          );
          return;
        }

        if (clientMode === "jsonl") {
          process.stdout.write(`${JSON.stringify(message)}\n`);
          continue;
        }

        process.stdout.write(`Content-Length: ${payload.length}\r\n\r\n`);
        process.stdout.write(payload);
      }
    } catch (error) {
      finish(error instanceof Error ? error : new Error(String(error)));
    }
  });

  const shutdown = () => {
    socket.end();
  };

  process.once("SIGINT", shutdown);
  process.once("SIGTERM", shutdown);

  await new Promise((resolve, reject) => {
    rejectBridge = reject;
    socket.once("close", () => {
      if (finished) {
        resolve();
        return;
      }
      finished = true;
      resolve();
    });
    socket.once("error", (error) => finish(error));
  });
}
