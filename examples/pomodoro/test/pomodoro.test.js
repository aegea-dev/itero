import { afterEach, beforeEach, describe, expect, it, setDefaultTimeout } from "bun:test";
import { Workspace } from "./workspace.js";

setDefaultTimeout(30000);

describe("pomodoro", () => {
  let ws;
  beforeEach(() => (ws = new Workspace()));
  afterEach(() => ws.remove());

  it("tells the session has finished once its duration is over, then ends", () => {
    const started = Date.now();
    ws.emit("pomodoro.start", { duration: 2 });
    ws.pass();
    expect(Date.now() - started).toBeGreaterThanOrEqual(2000);
    expect(ws.shown()).toEqual([{ title: "Pomodoro", text: "Pomodoro session finished" }]);
    const [run] = ws.runs();
    expect(run).toMatchObject({ workflow: "pomodoro", status: "done" });
  });

  it("says what is wrong with a duration it cannot wait for, and tells nothing", () => {
    for (const duration of [undefined, 0, -5, 2.5, "beş"]) {
      ws.emit("pomodoro.start", duration === undefined ? {} : { duration });
    }
    ws.pass();
    const runs = ws.runs();
    expect(runs).toHaveLength(5);
    for (const run of runs) {
      expect(run.status).toBe("error");
      expect(JSON.stringify(ws.story(run))).toContain("duration must be a whole number of seconds above 0");
    }
    expect(ws.shown()).toEqual([]);
  });
});
