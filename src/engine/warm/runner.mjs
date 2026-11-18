// itero's runner for a warm Node action: runs the action's file again for
// each request, in this process, so the modules it requires or imports stay
// loaded. Speaks to itero's worker on fd 3: a request is one JSON line in,
// its end one JSON line out. What the action prints goes to the real stdout
// and stderr; a random mark on each ends a request's output. Node 18+.
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import Module from "node:module";
import { Readable } from "node:stream";
import { pathToFileURL } from "node:url";

if (typeof process.getActiveResourcesInfo !== "function") {
  process.stderr.write(`warm: Node 18 or later is needed, this is ${process.version}\n`);
  process.exit(1);
}
const action = process.argv[1];
const base = { ...process.env };
const root = process.cwd();
// stdout and stderr are made on first use and stay: made now, before any
// step is measured against them
process.stdout; process.stderr;

class Exit extends Error {
  constructor(code) { super("exit"); this.code = code; }
}
const realExit = process.exit.bind(process);
let exitCode = null, failed = null;
process.exit = (code) => { throw new Exit(code ?? process.exitCode ?? 0); };
const caught = (e) => { if (e instanceof Exit) exitCode = e.code; else failed = e; };
process.on("uncaughtException", caught);
process.on("unhandledRejection", caught);

// a step's stdin read as fd 0: the step's own, not the runner's
let stdinBytes = Buffer.alloc(0), stdinAt = 0;
const realReadFileSync = fs.readFileSync;
fs.readFileSync = function (p, opts) {
  if (p === 0 || p === "/dev/stdin") {
    const rest = stdinBytes.subarray(stdinAt);
    stdinAt = stdinBytes.length;
    const enc = typeof opts === "string" ? opts : opts && opts.encoding;
    return enc ? rest.toString(enc) : Buffer.from(rest);
  }
  return realReadFileSync.apply(this, arguments);
};
const realReadSync = fs.readSync;
fs.readSync = function (fd, buffer, a, b) {
  if (fd === 0) {
    const offset = typeof a === "number" ? a : (a && a.offset) || 0;
    const length = typeof b === "number" ? b : (a && a.length) || buffer.length - offset;
    const n = stdinBytes.copy(buffer, offset, stdinAt, Math.min(stdinBytes.length, stdinAt + length));
    stdinAt += n;
    return n;
  }
  return realReadSync.apply(this, arguments);
};

// an uncaught error as node prints it: where, the line, the error and its
// stack (the action's frames; this runner's left out), node's version
function asNode(e) {
  if (!(e && e.stack)) return `Uncaught ${String(e)}\n\nNode.js ${process.version}\n`;
  const stack = e.stack.split("\n").filter((l) => !l.includes("[eval")).join("\n").replace(/\?run=\d+/g, "");
  const m = /\((?:file:\/\/)?(\/[^()]+?):(\d+):\d+\)|at (?:file:\/\/)?(\/[^:\s]+?):(\d+):\d+/.exec(stack);
  let head = "";
  if (m) {
    const file = m[1] || m[3], line = Number(m[2] || m[4]);
    try { head = `${file}:${line}\n${realReadFileSync(file, "utf8").split("\n")[line - 1]}\n^\n\n`; } catch {}
  }
  return `${head}${stack}\n\nNode.js ${process.version}\n`;
}

// what is running, by kind: {Timeout: 1, PipeWrap: 2, ...}
function running() {
  const kinds = {};
  for (const k of process.getActiveResourcesInfo()) if (k !== "Immediate") kinds[k] = (kinds[k] || 0) + 1;
  return kinds;
}
// the runner's own, when nothing of a step is left: of each kind, the
// least seen at a step's start (one may start while the runner's last
// writes still finish)
const idle = {};
function seeIdle() {
  const now = running();
  for (const k of new Set([...Object.keys(idle), ...Object.keys(now)])) idle[k] = Math.min(idle[k] ?? Infinity, now[k] || 0);
}
async function settled() {
  // done when what the step started has ended, as a fresh node exits then:
  // no kind above the runner's own
  for (;;) {
    await new Promise((r) => setImmediate(r));
    if (exitCode !== null || failed) return;
    const now = running();
    if (Object.keys(now).every((k) => now[k] <= (idle[k] || 0))) return;
    await new Promise((r) => setTimeout(r, 1));
  }
}

let n = 0;
async function run(req) {
  n += 1;
  for (const k of Object.keys(process.env)) delete process.env[k];
  Object.assign(process.env, base, req.env);
  process.chdir(root);
  process.argv = [process.execPath, action];
  process.exitCode = undefined;
  exitCode = null; failed = null;
  stdinBytes = Buffer.from(req.stdin); stdinAt = 0;
  Object.defineProperty(process, "stdin", { value: Readable.from([Buffer.from(req.stdin)]), configurable: true, writable: true });
  seeIdle();
  const src = realReadFileSync(action, "utf8").replace(/^#!.*/, "");
  try {
    let esm = false;
    // as node tells them apart: CommonJS, unless its syntax is a module's
    try { new vm.Script(Module.wrap(src), { filename: action }); } catch (e) { esm = e instanceof SyntaxError; if (!esm) throw e; }
    if (esm) {
      await import(pathToFileURL(action).href + "?run=" + n);
    } else {
      const m = new Module(action, null);
      m.filename = action;
      m.paths = Module._nodeModulePaths(path.dirname(action));
      m._compile(src, action);
    }
    await settled();
  } catch (e) { caught(e); }
  if (failed) { process.stderr.write(asNode(failed)); return 1; }
  return exitCode ?? process.exitCode ?? 0;
}

const requests = fs.createReadStream(null, { fd: 3, autoClose: false });
let buf = "", queue = Promise.resolve();
requests.on("data", (d) => {
  buf += d.toString();
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i);
    buf = buf.slice(i + 1);
    queue = queue.then(async () => {
      const req = JSON.parse(line);
      const code = await run(req);
      const mark = "\n" + req.mark + "\n";
      await new Promise((r) => process.stdout.write(mark, r));
      await new Promise((r) => process.stderr.write(mark, r));
      fs.writeSync(3, JSON.stringify({ exit: code & 255, left: null }) + "\n");
    });
  }
});
requests.on("end", () => realExit(0));
