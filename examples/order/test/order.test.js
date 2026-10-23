import { expect } from "chai";
import { Workspace } from "./workspace.js";

describe("order", function () {
  this.timeout(20000);
  let ws;
  beforeEach(() => (ws = new Workspace()));
  afterEach(() => ws.remove());

  const placed = { order: 43, email: "ayşe@örnek.test" };

  it("waits for its payment before anything ships", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    const [run] = ws.runs();
    expect(run.status).to.equal("waiting");
    expect(ws.state(run)).to.deep.include({ order: 43, payment_ref: "pay-43", stock: true });
    expect(ws.read(".itero/sent.log")).to.equal("");
  });

  it("ships once paid, and mails the tracking number", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-43", paid: true });
    ws.pass();
    const [run] = ws.runs();
    expect(run.status).to.equal("done");
    expect(ws.state(run)).to.deep.include({ paid: true, tracking: "TR-43" });
    expect(ws.read(".itero/sent.log")).to.equal("to ayşe@örnek.test: order 43, TR-43\n");
    expect(ws.read(".itero/results.log")).to.equal("order 43: done\n");
  });

  it("fails, and says why, when the payment is refused", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-43", paid: false });
    ws.pass();
    const [run] = ws.runs();
    expect(run.status).to.equal("failed");
    expect(ws.state(run)).to.deep.include({ success: false, why: "not paid" });
    expect(ws.read(".itero/sent.log")).to.equal("to ayşe@örnek.test: order 43, not shipped\n");
    expect(ws.read(".itero/results.log")).to.equal("order 43: failed: not paid\n");
  });

  it("is not reached by another order's payment", () => {
    ws.emit("order.placed", placed);
    ws.pass();
    ws.emit("payment.received", { payment_ref: "pay-44", paid: true });
    ws.pass();
    expect(ws.runs()[0].status).to.equal("waiting");
  });
});
