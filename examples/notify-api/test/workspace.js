// A throwaway copy of this workspace, its daemon running on a free port:
// the app's side is plain HTTP. Notifications are written to a file
// (ITERO_NOTIFY_LOG) instead of shown, and a click is told the way
// Itero.app tells it. itero is taken from PATH, or from $ITERO_BIN.
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = join(dirname(fileURLToPath(import.meta.url)), "..");
const bin = process.env.ITERO_BIN ?? "itero";

/** A port nothing listens on. */
function freePort() {
  const s = Bun.serve({ port: 0, fetch: () => new Response() });
  const port = s.port;
  s.stop(true);
  return port;
}

/** Waits up to 15s for `done()`, which may be async. */
export async function until(what, done) {
  const end = Date.now() + 15000;
  while (!(await done())) {
    if (Date.now() > end) throw new Error(`waited 15s for ${what}`);
    await Bun.sleep(100);
  }
}

export class Workspace {
  constructor() {
    this.dir = mkdtempSync(join(tmpdir(), "itero-test-"));
    cpSync(here, this.dir, {
      recursive: true,
      filter: (p) => !/\/(\.itero|node_modules)(\/|$)/.test(p),
    });
    this.port = freePort();
    const f = join(this.dir, "itero.config.json");
    const m = JSON.parse(readFileSync(f, "utf8"));
    m.port = this.port;
    writeFileSync(f, JSON.stringify(m, null, 2));
    this.env = {
      ...process.env,
      ITERO_NO_SECRET_STORE: "1",
      ITERO_NOTIFY_LOG: join(this.dir, "shown.log"),
    };
  }

  /** Starts `itero daemon`, and waits until its API answers. */
  async start() {
    this.daemon = Bun.spawn([bin, "daemon"], { cwd: this.dir, env: this.env, stdout: "ignore", stderr: "ignore" });
    await until("the daemon's API", async () => {
      try {
        return (await fetch(`${this.api}/health`)).ok;
      } catch {
        return false;
      }
    });
  }

  get api() {
    return `http://127.0.0.1:${this.port}`;
  }

  /** The notifications shown: each its title, text and fields by name. */
  shown() {
    const f = this.env.ITERO_NOTIFY_LOG;
    if (!existsSync(f)) return [];
    return readFileSync(f, "utf8")
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        const [title, text, ...fields] = line.split("\t");
        return { title, text, ...Object.fromEntries(fields.map((f) => f.split(/=(.*)/s).slice(0, 2))) };
      });
  }

  /** A click on a notification, as Itero.app tells it. */
  click(click) {
    execFileSync(bin, ["notification-clicked", JSON.stringify(click)], { cwd: this.dir, env: this.env });
  }

  remove() {
    this.daemon?.kill();
    rmSync(this.dir, { recursive: true, force: true });
  }
}
