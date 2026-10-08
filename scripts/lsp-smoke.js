const { spawn } = require("child_process");
const path = require("path");

const VN = process.argv[2] || path.join(__dirname, "..", "target", "quick", "vn.exe");
const FILE = process.argv[3];
const LINE = parseInt(process.argv[4] || "5", 10);
const CHAR = parseInt(process.argv[5] || "8", 10);

const uri = "file:///" + FILE.replace(/\\/g, "/").replace(/^([A-Za-z]):/, "$1:");
const fs = require("fs");
const text = fs.readFileSync(FILE, "utf8");

const child = spawn(VN, ["lsp"], { stdio: ["pipe", "pipe", "inherit"] });
let buf = Buffer.alloc(0);
let seq = 0;
const pending = new Map();

function send(method, params, id) {
  const msg = { jsonrpc: "2.0", method };
  if (id !== undefined) msg.id = id;
  if (params !== undefined) msg.params = params;
  const body = Buffer.from(JSON.stringify(msg));
  child.stdin.write(
    Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`), body])
  );
  return id;
}
function req(method, params) {
  const id = ++seq;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    send(method, params, id);
  });
}
function notify(method, params) {
  send(method, params);
}

child.stdout.on("data", (chunk) => {
  buf = Buffer.concat([buf, chunk]);
  for (;;) {
    const h = buf.indexOf("\r\n\r\n");
    if (h < 0) break;
    const m = /Content-Length: (\d+)/i.exec(buf.subarray(0, h).toString());
    if (!m) break;
    const n = parseInt(m[1], 10);
    if (buf.length < h + 4 + n) break;
    const body = JSON.parse(buf.subarray(h + 4, h + 4 + n).toString());
    buf = buf.subarray(h + 4 + n);
    if (body.id !== undefined && pending.has(body.id)) {
      pending.get(body.id)(body);
      pending.delete(body.id);
    } else if (body.method) {
      console.log(`N ${body.method} ${JSON.stringify(body.params).slice(0, 160)}`);
    }
  }
});

(async () => {
  const caps = await req("initialize", { processId: null, rootUri: null, capabilities: {} });
  const c = caps.result.capabilities;
  console.log("CAPS typeHierarchy=" + JSON.stringify(c.typeHierarchyProvider));
  console.log("CAPS semanticTokens=" + JSON.stringify(c.semanticTokensProvider && { full: !!c.semanticTokensProvider.full, range: !!c.semanticTokensProvider.range }));
  notify("initialized", {});
  notify("textDocument/didOpen", {
    textDocument: { uri, languageId: "varn", version: 1, text },
  });
  await new Promise((r) => setTimeout(r, 2500));
  const sem = await req("textDocument/semanticTokens/full", { textDocument: { uri } });
  const data = sem.result && sem.result.data;
  console.log("SEMANTIC tokens=" + (data ? data.length : "NULL"));
  const prep = await req("textDocument/prepareTypeHierarchy", {
    textDocument: { uri },
    position: { line: LINE, character: CHAR },
  });
  console.log("PREPARE " + JSON.stringify(prep.result));
  if (prep.result && prep.result[0]) {
    const sup = await req("typeHierarchy/supertypes", { item: prep.result[0] });
    console.log("SUPERS " + JSON.stringify(sup.result));
  }
  const hover = await req("textDocument/hover", {
    textDocument: { uri },
    position: { line: LINE, character: CHAR },
  });
  console.log("HOVER " + JSON.stringify(hover.result && hover.result.contents).slice(0, 200));
  child.kill();
  process.exit(0);
})().catch((e) => {
  console.error("SMOKE_FAIL", e);
  child.kill();
  process.exit(1);
});
setTimeout(() => { console.error("TIMEOUT"); child.kill(); process.exit(2); }, 25000).unref();
