---
title: Testing
description: Testing a workflow with #ai steps without a model.
---

A test of a workflow with `#ai` steps should not need a model: a model
is large, slow to load, and its answers vary. Test the workflow instead:
given an answer, does it do the right thing?

## A stand-in for itero-ai

Set `ITERO_AI_SOCKET` in the daemon's environment, and `#ai` steps ask
whatever answers on that socket; no itero-ai is started. A small server
of your test's answers there, with fixed answers in OpenAI's shape.

`#ai` always asks for a streamed answer, so the stand-in streams: one
chunk with the content, one with `finish_reason`, then `[DONE]`. With
Bun, which serves on a Unix socket:

```js
// fake-ai.js: answers the label in the file $ANSWER, on the socket $SOCKET
import { readFileSync } from "node:fs";

Bun.serve({
  unix: process.env.SOCKET,
  async fetch(req) {
    const answer = readFileSync(process.env.ANSWER, "utf8").trim();
    const chunk = (delta, finish = null) =>
      `data: ${JSON.stringify({ object: "chat.completion.chunk",
                                choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
    const body = chunk({ content: JSON.stringify(answer) }) + chunk({}, "stop") + "data: [DONE]\n\n";
    return new Response(body, { headers: { "content-type": "text/event-stream" } });
  },
});
```

The content is what the model would write:

- for a plain `text` answer, the text;
- for `choose`, a label as JSON: `"urgent"`, with its quotes;
- for `fields`, an object as JSON: `{"urgent": true, "amount": 1250}`.

Run the stand-in in a process of its own: a test that waits on itero
would otherwise keep it from answering.

## In a test

```js
const server = Bun.spawn(["bun", "fake-ai.js"], {
  env: { ...process.env, SOCKET: socket, ANSWER: answerFile },
});
while (!existsSync(socket)) await Bun.sleep(20);
process.env.ITERO_AI_SOCKET = socket;   // the daemon the test starts inherits it
```

Then each test writes the answer it wants, sends the event, and checks
what the workflow did. The stand-in can also keep each request it gets,
so a test can check what the model was asked: the `model`, the
`messages`, and the `response_format` that holds it to the labels.

The [`ai-mail` example](https://github.com/aegea-dev/itero/tree/main/examples/ai-mail)
does all of this: `test/fake-ai.js`, and `test/sort.test.js` with an
urgent mail and one for later.

## What it does not test

The stand-in says nothing about the model's real answers: whether
Qwen3.5-4B sorts your mails well. Check that by hand, with the real
model, on mails of your own, and keep the ones it gets wrong as cases
for the prompt.
