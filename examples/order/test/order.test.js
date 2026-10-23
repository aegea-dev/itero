import { afterEach, beforeEach, describe, expect, it, setDefaultTimeout } from "bun:test";
import { Workspace } from "./workspace.js";

setDefaultTimeout(20000);

describe("order", () => {
  let ws;
  beforeEach(() => (ws = new Workspace()));
  afterEach(() => ws.remove());

  const placed = { order: 43, email: "ayşe@örnek.test" };

  it("waits for its payment before anything ships", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    const [run] = ws.runs();
    expect(run.status).toBe("waiting");
    expect(ws.state(run)).toMatchObject({ order: 43, payment_ref: "pay-43", stock: true });
    expect(ws.read(".itero/sent.log")).toBe("");
  });

  it("ships once paid, and mails the tracking number", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-43", paid: true });
    ws.pass();
    const [run] = ws.runs();
    expect(run.status).toBe("done");
    expect(ws.state(run)).toMatchObject({ paid: true, tracking: "TR-43" });
    expect(ws.read(".itero/sent.log")).toBe("to ayşe@örnek.test: order 43, TR-43\n");
    expect(ws.read(".itero/results.log")).toBe("order 43: done\n");
  });

  it("fails, and says why, when the payment is refused", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-43", paid: false });
    ws.pass();
    const [run] = ws.runs();
    expect(run.status).toBe("failed");
    expect(ws.state(run)).toMatchObject({ success: false, why: "not paid" });
    expect(ws.read(".itero/sent.log")).toBe("to ayşe@örnek.test: order 43, not shipped\n");
    expect(ws.read(".itero/results.log")).toBe("order 43: failed: not paid\n");
  });

  it("is not reached by another order's payment", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-44", paid: true });
    ws.pass();
    expect(ws.runs()[0].status).toBe("waiting");
  });
});
