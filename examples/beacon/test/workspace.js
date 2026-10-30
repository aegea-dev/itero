// A throwaway copy of this workspace, driven through itero's command line:
// emit a message, let the daemon make one pass (at a time the test
// chooses), and read the runs back. itero is taken from PATH, or from
// $ITERO_BIN.
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = join(dirname(fileURLToPath(import.meta.url)), "..");
const bin = process.env.ITERO_BIN ?? "itero";

export class Workspace {
  constructor() {
    this.dir = mkdtempSync(join(tmpdir(), "itero-test-"));
    cpSync(here, this.dir, {
      recursive: true,
      filter: (p) => !/\/(\.itero|node_modules)(\/|$)/.test(p),
    });
  }

  itero(...args) {
    return execFileSync(bin, args, {
      cwd: this.dir,
      encoding: "utf8",
      // itero's notes on stderr stay off the test output; a failing command
      // still throws with them
      stdio: ["ignore", "pipe", "pipe"],
      // no OS secret store, and notifications go nowhere
      env: { ...process.env, ITERO_NO_SECRET_STORE: "1", ITERO_NOTIFIER: "/usr/bin/true" },
    });
  }

  emit(type, data) {
    this.itero("emit", type, JSON.stringify(data));
  }

  /** One daemon pass, as if it were `now` (an RFC 3339 time). */
  pass(now = new Date().toISOString().replace(/\.\d+Z$/, "Z")) {
    this.itero("daemon", "--once", "--now", now);
  }

  /** The runs, newest first: { id, name, workflow, status, ... }. */
  runs() {
    return JSON.parse(this.itero("workflow", "runs", "--json"));
  }

  /** A run's state, from its story. */
  state(run) {
    return JSON.parse(this.itero("workflow", "runs", run.id, "--json")).state;
  }

  /** What `step` (like "#changed") printed in a run: its result. */
  output(run, step) {
    const story = JSON.parse(this.itero("workflow", "runs", run.id, "--json"));
    return story.steps.findLast((s) => s.what === "done" && s.step === step)?.data;
  }

  /** Changes the manifest: `edit` gets it as an object and changes it. */
  manifest(edit) {
    const f = join(this.dir, "itero.config.json");
    const m = JSON.parse(readFileSync(f, "utf8"));
    edit(m);
    writeFileSync(f, JSON.stringify(m, null, 2));
  }

  /** Changes a workflow: `edit` gets its file as an object and changes it. */
  workflow(name, edit) {
    const f = join(this.dir, "workflows", `${name}.json`);
    const w = JSON.parse(readFileSync(f, "utf8"));
    edit(w);
    writeFileSync(f, JSON.stringify(w, null, 2));
  }

  read(rel) {
    const f = join(this.dir, rel);
    return existsSync(f) ? readFileSync(f, "utf8") : "";
  }

  remove() {
    rmSync(this.dir, { recursive: true, force: true });
  }
}
