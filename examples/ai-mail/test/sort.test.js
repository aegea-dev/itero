import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, setDefaultTimeout } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Workspace } from "./workspace.js";

setDefaultTimeout(20000);

// A stand-in for itero-ai (fake-ai.js), in its own process: no model is
// needed. The answer it gives is the file `answerFile`'s.
const dir = mkdtempSync(join(tmpdir(), "ai-"));
const socket = join(dir, "ai.sock");
const answerFile = join(dir, "answer");
const askedFile = join(dir, "asked.jsonl");
const answer = (label) => writeFileSync(answerFile, label);
const asked = () =>
  existsSync(askedFile) ? readFileSync(askedFile, "utf8").trim().split("\n").map((l) => JSON.parse(l)) : [];
let server;

beforeAll(async () => {
  answer("urgent");
  server = Bun.spawn(["bun", join(import.meta.dir, "fake-ai.js")], {
    env: { ...process.env, SOCKET: socket, ANSWER: answerFile, ASKED: askedFile },
  });
  while (!existsSync(socket)) await Bun.sleep(20);
  process.env.ITERO_AI_SOCKET = socket;
});

afterAll(() => {
  server.kill();
  rmSync(dir, { recursive: true, force: true });
});

describe("ai-mail", () => {
  let ws;
  beforeEach(() => {
    ws = new Workspace();
    process.env.ITERO_NOTIFY_LOG = join(ws.dir, "bildirimler.log");
    rmSync(askedFile, { force: true });
  });
  afterEach(() => ws.remove());

  const mail = { from: "izleme", subject: "Veritabanı çöktü", body: "Müşteriler ödeme yapamıyor." };

  it("tells you of an urgent mail", () => {
    answer("urgent");
    ws.emit("mail.received", mail);
    ws.pass();
    const [run] = ws.runs();
    expect(run.status).toBe("done");
    expect(ws.output(run, "#ai")).toBe("urgent");
    expect(ws.read("bildirimler.log")).toBe("Urgent: Veritabanı çöktü\tfrom izleme\n");
    // the model is asked with the mail, held to the three labels
    const [req] = asked();
    expect(req.model).toBe("unsloth/Qwen3.5-4B-GGUF");
    expect(req.messages[1].content).toStartWith("From izleme, subject: Veritabanı çöktü");
    expect(req.response_format.json_schema.schema.enum).toEqual(["urgent", "later", "spam"]);
  });

  it("keeps quiet about one for later", () => {
    answer("later");
    ws.emit("mail.received", { ...mail, subject: "Toplantı perşembeye kaydı" });
    ws.pass();
    const [run] = ws.runs();
    expect(run.status).toBe("done");
    expect(ws.output(run, "#ai")).toBe("later");
    expect(ws.read("bildirimler.log")).toBe("");
  });
});
