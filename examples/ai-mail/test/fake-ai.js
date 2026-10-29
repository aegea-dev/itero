// A stand-in for itero-ai, in its own process (the tests block on itero):
// OpenAI's chat API on the Unix socket $SOCKET, answering the label in the
// file $ANSWER, streamed as itero-ai does, and adding each request to the
// file $ASKED, a JSON object a line.
import { appendFileSync, readFileSync } from "node:fs";

Bun.serve({
  unix: process.env.SOCKET,
  async fetch(req) {
    appendFileSync(process.env.ASKED, JSON.stringify(await req.json()) + "\n");
    const answer = readFileSync(process.env.ANSWER, "utf8").trim();
    const chunk = (delta, finish = null) =>
      `data: ${JSON.stringify({ object: "chat.completion.chunk", choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
    const body = chunk({ content: JSON.stringify(answer) }) + chunk({}, "stop") + "data: [DONE]\n\n";
    return new Response(body, { headers: { "content-type": "text/event-stream" } });
  },
});
