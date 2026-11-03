import { afterEach, beforeEach, describe, expect, it, setDefaultTimeout } from "bun:test";
import { Workspace } from "./workspace.js";

setDefaultTimeout(20000);

describe("order", () => {
  let ws;
  beforeEach(() => (ws = new Workspace()));
  afterEach(() => ws.remove());

  const placed = { order: 43, email: "ayşe@örnek.test" };
  // the order's run: order-process, a workflow used as a step, is a run
  // of its own
  const order = () => ws.runs().find((r) => r.workflow === "order");

  it("waits for its payment before anything ships", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    const run = order();
    expect(run.status).toBe("waiting");
    expect(ws.output(run, "#check-payment")).toBeUndefined();
    expect(ws.output(run, "#check-stock")).toEqual({ stock: true });
    expect(ws.read(".itero/sent.log")).toBe("");
  });

  it("ships once paid, and mails the tracking number", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-43", paid: true });
    ws.pass();
    const run = order();
    expect(run.status).toBe("done");
    // each step's result, by its name
    expect(ws.state(run).steps).toMatchObject({
      "check-payment": { paid: true },
      "check-stock": { stock: true },
      "order-process": { output: { tracking: "TR-43" }, status: "done" },
    });
    expect(ws.read(".itero/sent.log")).toBe("to ayşe@örnek.test: order 43, TR-43\n");
    expect(ws.read(".itero/results.log")).toBe("order 43: done\n");
  });

  it("fails, and says why, when the payment is refused", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-43", paid: false });
    ws.pass();
    const run = order();
    expect(run.status).toBe("failed");
    expect(ws.state(run).steps["order-process"]).toMatchObject({
      output: { success: false, why: "not paid" },
      status: "failed",
    });
    expect(ws.read(".itero/sent.log")).toBe("to ayşe@örnek.test: order 43, not shipped\n");
    expect(ws.read(".itero/results.log")).toBe("order 43: failed: not paid\n");
  });

  it("is not reached by another order's payment", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-44", paid: true });
    ws.pass();
    expect(order().status).toBe("waiting");
  });
});
