const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");

const VN = process.argv[2];
const FILE = process.argv[3];
const LINE = parseInt(process.argv[4] || "3", 10);
const CHAR = parseInt(process.argv[5] || "7", 10);

const uri = "file:///" + FILE.replace(/\\/g, "/").replace(/^([A-Za-z]):/, "$1:");
const text = fs.readFileSync(FILE, "utf8");

const child = spawn(VN, ["lsp"], { stdio: ["pipe", "pipe", "ignore"] });
let buf = Buffer.alloc(0);
let seq = 0;
const pending = new Map();

function send(method, params, id) {
  const msg = { jsonrpc: "2.0", method };
  if (id !== undefined) msg.id = id;
  if (params !== undefined) msg.params = params;
  const body = Buffer.from(JSON.stringify(msg));
  child.stdin.write(Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`), body]));
}
function req(method, params) {
  const id = ++seq;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    send(method, params, id);
  });
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
    }
  }
});

function short(v) {
  const s = JSON.stringify(v);
  return s.length > 600 ? s.slice(0, 600) + "..." : s;
}

(async () => {
  await req("initialize", { processId: null, rootUri: null, capabilities: {} });
  child.stdin.write("");
  const { promisify } = require("util");
  send("initialized", {});
  send("textDocument/didOpen", {
    textDocument: { uri, languageId: "varn", version: 1, text },
  });
  await new Promise((r) => setTimeout(r, 2500));
  const prep = await req("textDocument/prepareTypeHierarchy", {
    textDocument: { uri },
    position: { line: LINE, character: CHAR },
  });
  console.log("PREPARE " + short(prep.result));
  const item = prep.result && prep.result[0];
  if (item) {
    const sup = await req("typeHierarchy/supertypes", { item });
    console.log("SUPERS " + short(sup.result));
    const sub = await req("typeHierarchy/subtypes", { item: (sup.result && sup.result[0]) || item });
    console.log("SUBS " + short(sub.result));
  }
  child.kill();
  process.exit(0);
})().catch((e) => {
  console.error("SMOKE_FAIL", e);
  child.kill();
  process.exit(1);
});
setTimeout(() => {
  console.error("TIMEOUT");
  child.kill();
  process.exit(2);
}, 25000).unref();
