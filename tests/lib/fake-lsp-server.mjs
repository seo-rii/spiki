let buffer = Buffer.alloc(0);
const documents = new Map();

function writeMessage(message) {
  const payload = Buffer.from(JSON.stringify(message), "utf8");
  process.stdout.write(`Content-Length: ${payload.length}\r\n\r\n`);
  process.stdout.write(payload);
}

process.stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (true) {
    const headerEnd = buffer.indexOf("\r\n\r\n");
    if (headerEnd === -1) {
      return;
    }

    const header = buffer.subarray(0, headerEnd).toString("utf8");
    const contentLengthLine = header
      .split(/\r?\n/u)
      .find((line) => line.toLowerCase().startsWith("content-length:"));
    if (!contentLengthLine) {
      process.exit(1);
    }

    const length = Number(contentLengthLine.split(":")[1].trim());
    if (!Number.isInteger(length) || length < 0) {
      process.exit(1);
    }

    const bodyStart = headerEnd + 4;
    if (buffer.length < bodyStart + length) {
      return;
    }

    const payload = buffer.subarray(bodyStart, bodyStart + length);
    buffer = buffer.subarray(bodyStart + length);
    const message = JSON.parse(payload.toString("utf8"));

    if (message.method === "initialize") {
      writeMessage({
        jsonrpc: "2.0",
        id: message.id,
        result: {
          capabilities: {
            definitionProvider: true,
            textDocumentSync: 1
          }
        }
      });
      continue;
    }

    if (message.method === "initialized") {
      continue;
    }

    if (message.method === "workspace/didChangeConfiguration") {
      continue;
    }

    if (message.method === "textDocument/didOpen") {
      documents.set(message.params.textDocument.uri, message.params.textDocument.text);
      continue;
    }

    if (message.method === "textDocument/didChange") {
      const changes = message.params.contentChanges ?? [];
      documents.set(message.params.textDocument.uri, changes[changes.length - 1]?.text ?? "");
      continue;
    }

    if (message.method === "textDocument/definition") {
      const uri = message.params.textDocument.uri;
      const text = documents.get(uri) ?? "";
      const matchIndex = text.indexOf("answer");
      if (matchIndex === -1) {
        writeMessage({
          jsonrpc: "2.0",
          id: message.id,
          result: null
        });
        continue;
      }

      const before = text.slice(0, matchIndex);
      const line = before.split("\n").length - 1;
      const character = before.split("\n").at(-1)?.length ?? 0;
      writeMessage({
        jsonrpc: "2.0",
        id: message.id,
        result: {
          uri,
          range: {
            start: { line, character },
            end: { line, character: character + "answer".length }
          }
        }
      });
      continue;
    }

    if (message.id != null) {
      writeMessage({
        jsonrpc: "2.0",
        id: message.id,
        result: null
      });
    }
  }
});

process.stdin.resume();
