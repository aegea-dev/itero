import { afterEach, beforeEach, describe, expect, it, setDefaultTimeout } from "bun:test";
import { Workspace } from "./workspace.js";

setDefaultTimeout(30000);

describe("beacon", () => {
  let ws;
  beforeEach(() => {
    ws = new Workspace();
    // a site nothing answers on, so the test needs no network; the disk
    // workflow is off, so only the sites run
    ws.manifest((m) => {
      m.workflow.sites.settings.sites = ["http://127.0.0.1:9"];
      m.workflow.disk.off = true;
    });
  });
  afterEach(() => ws.remove());

  // the daemon sees a schedule first, then starts it at its next slot
  const at = (minute) => `2026-10-23T10:${String(minute).padStart(2, "0")}:05Z`;

  it("checks the sites every minute and tells what it watches", () => {
    ws.pass(at(0));
    ws.pass(at(1));
    const [run] = ws.runs();
    expect(run).toMatchObject({ workflow: "sites", status: "done" });
    expect(ws.state(run)).toMatchObject({
      status: { "http://127.0.0.1:9": "down" },
      text: "http://127.0.0.1:9: down",
    });
  });

  it("tells nothing when nothing changed", () => {
    ws.pass(at(0));
    ws.pass(at(1));
    ws.pass(at(2));
    const runs = ws.runs();
    expect(runs).toHaveLength(2);
    expect(ws.state(runs[0])).not.toHaveProperty("text");
  });

  it("tells again when a site's state changes", () => {
    ws.pass(at(0));
    ws.pass(at(1));
    ws.manifest((m) => m.workflow.sites.settings.sites.push("http://127.0.0.1:19"));
    ws.pass(at(2));
    expect(ws.state(ws.runs()[0]).text).toBe("http://127.0.0.1:19: down");
  });
});
