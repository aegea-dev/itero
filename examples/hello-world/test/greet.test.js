import { afterEach, beforeEach, describe, expect, it, setDefaultTimeout } from "bun:test";
import { Workspace } from "./workspace.js";

setDefaultTimeout(30000);

describe("hello-world", () => {
  let ws;
  beforeEach(() => (ws = new Workspace()));
  afterEach(() => ws.remove());

  it("greets the event's name with a notification, then ends", () => {
    ws.emit("greet.someone", { name: "Ayşe" });
    ws.pass();
    expect(ws.shown()).toEqual([{ title: "itero", text: "Merhaba, Ayşe!" }]);
    const [run] = ws.runs();
    expect(run).toMatchObject({ workflow: "greet", status: "done" });
  });
});
