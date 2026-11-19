// A throwaway copy of this workspace, driven through itero's command line:
// send a message, let the daemon make one pass, read the runs back.
// Notifications are written to a file (ITERO_NOTIFY_LOG) instead of shown.
// itero is taken from PATH, or from $ITERO_BIN.
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
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
    this.notes = join(this.dir, "shown.log");
  }

  itero(...args) {
    return execFileSync(bin, args, {
      cwd: this.dir,
      encoding: "utf8",
      // itero's notes on stderr stay off the test output; a failing command
      // still throws with them
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, ITERO_NO_SECRET_STORE: "1", ITERO_NOTIFY_LOG: this.notes },
    });
  }

  emit(type, data) {
    this.itero("ws", "emit", type, JSON.stringify(data));
  }

  /** One daemon pass: it ends once its steps have. */
  pass() {
    this.itero("ws", "daemon", "--once");
  }

  /** The runs, newest first: { id, name, workflow, status, ... }. */
  runs() {
    return JSON.parse(this.itero("ws", "wf", "runs", "--json"));
  }

  /** A run's story: its state and steps. */
  story(run) {
    return JSON.parse(this.itero("ws", "wf", "runs", run.id, "--json"));
  }

  /** The notifications shown: each its title and text. */
  shown() {
    if (!existsSync(this.notes)) return [];
    return readFileSync(this.notes, "utf8")
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        const [title, text] = line.split("\t");
        return { title, text };
      });
  }

  remove() {
    rmSync(this.dir, { recursive: true, force: true });
  }
}
