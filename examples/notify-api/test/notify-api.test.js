import { afterEach, beforeEach, describe, expect, it, setDefaultTimeout } from "bun:test";
import { Workspace, until } from "./workspace.js";

setDefaultTimeout(30000);

describe("notify-api", () => {
  let ws;
  let app;
  let posted;
  beforeEach(async () => {
    ws = new Workspace();
    posted = [];
    // the app: where the answer is posted
    app = Bun.serve({
      port: 0,
      fetch: async (req) => {
        posted.push(await req.json());
        return new Response("ok");
      },
    });
    await ws.start();
  });
  afterEach(() => {
    app.stop(true);
    ws.remove();
  });

  it("shows what the app asks, and posts the answer back when clicked", async () => {
    const asked = await fetch(`${ws.api}/events`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        type: "app.notify",
        data: {
          title: "Dağıtılsın mı?",
          text: "3 değişiklik",
          buttons: ["Dağıt", "Vazgeç"],
          request: "istek-1",
          reply_url: `http://127.0.0.1:${app.port}/notified`,
        },
      }),
    });
    expect(asked.status).toBe(202);

    await until("the notification", () => ws.shown().length === 1);
    const [shown] = ws.shown();
    expect(shown).toMatchObject({ title: "Dağıtılsın mı?", text: "3 değişiklik", buttons: '["Dağıt","Vazgeç"]' });
    // nothing is posted while it waits
    await Bun.sleep(1500);
    expect(posted).toEqual([]);

    ws.click({ notification: shown.notification, answer: "Dağıt", workflow: "app-notify" });
    await until("the answer, posted to the app", () => posted.length === 1);
    // the run's state, posted by the workflow's success step
    expect(posted[0]).toMatchObject({ answer: "Dağıt", request: "istek-1", notification: shown.notification });
  });
});
