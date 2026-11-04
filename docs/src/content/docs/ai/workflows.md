---
title: "#ai in a workflow"
description: The built-in #ai asks a model on your own machine, or one behind a URL, for a text or typed fields.
---

The built-in action `#ai` asks a language model in a step: to sum up a
mail, to sort it, to pull the amount and the date out of an invoice. The
model runs on your own machine, through [itero-ai](/ai/), and nothing
leaves the machine; or it is any model behind an OpenAI-compatible
[URL](#a-model-behind-a-url), on this machine or another.

## `#ai` ■

```json
{ "#ai": { "args": { "message": "Sum up this mail in one line: $event.body" } } }
```

`$output` is the answer, as text. The model is this machine's: the first
one you add with `itero ai models add` is used until you
[choose another](#values-a-step-leaves-out).

To sort, say the labels, and what each means, in `system` or `message`:

```json
{ "#ai": { "args": {
    "system": "Answer with one word only: urgent (someone must act today), later (it can wait) or spam (nobody needs it).",
    "message": "$event.body" } } }
```

A long prompt can live in a file, passed as it is:
`"system": { "file": "prompts/sort.md" }`
([Args](/guides/workflows/#args-what-a-step-gives-its-action)).

**Fields** hold the model to a form, so the next steps can rely on it:
`$output` is an object with exactly those fields, of those types.

```json
{ "#ai": { "args": { "message": "$event.body",
                     "fields": { "urgent": "boolean", "amount": "number", "due": "date" } } } }
```

A condition reads them as any step's result:

```json
{ "#notify": { "on": [{ "$output.urgent": true }],
               "args": { "title": "Urgent", "text": "$event.subject" } } }
```

For `fields`, the model is held to the form in two ways: a JSON schema
lets it write nothing else, and a sentence added to `message` tells it
the form (`Answer as a JSON object: {"urgent": <true or false>, ...}.`).
A small model held by the schema alone, without being told, answers
poorly. itero checks every answer: a field missing or of the wrong type
fails the step.

## Its args

| Arg | Is |
|---|---|
| `url` | a model behind this address, not itero-ai ([A model behind a URL](#a-model-behind-a-url)) |
| `key` | the key that `url` needs, as a `$$NAME` |
| `model` | the model: `<org>/<repo>`, with `:<quantization>` or without ([Models](/ai/models/)); without it, the machine's. With a `url`, the id that server expects, or none for its default |
| `message` | what is asked; needed |
| `system` | the system message: who the model is to be, and how to answer |
| `fields` | names and their types, `text`, `number`, `integer`, `boolean` or `date`: the answer is an object of them |
| `thinking` | `true` lets the model think first, its thinking to the log, not into `$output`; `false` asks it not to ([Thinking](#thinking)) |
| `temperature`, `top_p`, `top_k`, `min_p`, `presence_penalty`, `max_tokens`, `seed` | sampling, sent as given |
| `context` | itero-ai's: tokens for the prompt and the answer together: 1,024 or more, up to what the model holds; 32,768 if not given |

Without a quantization, the model is the repo's only file, else its
`Q4_K_M`, else an error naming the ones there.

A larger `context` costs memory: about 500 MB for each 16,384 tokens on
[Qwen3.5-4B](/ai/qwen3.5-4b/#what-we-measured).

## Values a step leaves out

A step only needs `message`. For `url`, `key`, `model`, `thinking`,
sampling and `context`, the first of these that has a value wins:

1. the step's args;
2. this machine's defaults, set with
   [`itero ai defaults set`](/commands/ai/#defaults);
3. for itero-ai, the model's own: itero's table for the models it
   advises, else the sampling its file names;
4. for itero-ai, llama.cpp's own: `temperature` 0.8, `top_k` 40,
   `top_p` 0.95, `min_p` 0.05.

`itero ai defaults` shows each value and where it comes from. So a
machine can say which model it uses, and a step only says what differs:

```sh
itero ai defaults set '{"model": "unsloth/Qwen3.5-4B-GGUF"}'
itero ai defaults
```

## A model behind a URL

With a `url`, the step asks the model behind it, not itero-ai: OpenAI,
OpenRouter, Groq, an LM Studio or vLLM, on this machine or another. Any
server that speaks OpenAI's API:

```json
{ "#ai": { "args": { "url": "https://api.openai.com/v1/chat/completions",
                     "key": "$$OPENAI_API_KEY", "model": "gpt-6-astra",
                     "system": "Sort the mail of a small shop.", "message": "$event.body" } } }
```

The same through OpenRouter: `"url":
"https://openrouter.ai/api/v1/chat/completions"`, `"key":
"$$OPENROUTER_API_KEY"`, `"model": "openai/gpt-6-astra"`.

- **`url`** is the full address, used as written. Its path picks the
  API: ending in `/chat/completions`, Chat Completions; ending in
  `/responses`, OpenAI's Responses API. Any other path is an error from
  `itero ws check`.
- **`key`** is sent as `Authorization: Bearer <key>`. Write it as a
  `$$NAME`, a [secret](/guides/secrets/), so it is never in the
  workflow's file; `itero ws check` warns of a key written out that
  looks like a token (20 characters or more, letters and digits mixed,
  and not a UUID). A server on your own machine, such as LM Studio,
  needs none.
- **`model`** is the id that server expects, as written: OpenAI's is
  `gpt-6-astra`, OpenRouter's carries the maker, `openai/gpt-6-astra`.
  Without one, none is sent and the server decides: OpenRouter answers
  with your account's default model, OpenAI refuses. The `ai.answer`
  log line shows the model that answered.

`itero ai defaults set` takes `url`, `key` and `model` too, so a machine
can send every `#ai` step to one server. A `key` or `model` from the
defaults goes only with the defaults' `url`: a step that gives its own
`url` gives its own `key` and `model`, so a key is never sent to a
server it was not set for.

**What is sent:** only what the step and the machine's defaults set.
The values `itero ai defaults` shows from itero's table or llama.cpp
are itero-ai's, and are not sent; the server uses its own. `context` is
itero-ai's, and an error on a step with a `url`. `fields` goes as a JSON
schema in the API's own form, and in words too; the answer is checked
as for itero-ai. `"thinking": false` asks for the lowest effort the
model takes (`none`, else `minimal`, else `low`); `"thinking": true`
sends nothing, and the model thinks as it does by default.

A Responses request says `"store": false`, so the server keeps no copy
of it (OpenAI keeps one 30 days otherwise); itero's own is the
`ai.answer` line in the log.

**What a server refuses** it is not asked again: when a server answers
400 naming the schema, the effort or streaming, itero keeps that for
the url and model (in `ai-servers.json`, in itero's state folder) and
sends the same request again at once without it. From then on `fields`
goes in words only, still checked, and `"thinking": false` tries the
next lower effort.

**Failures are not retried:** a 429, a server error, a timeout or a
dropped connection fails the step with the server's status and message,
and the workflow's own error handling takes over.

`itero ws check` says, for each `#ai` step, where it sends:
`local: <model>` for itero-ai, `local: <url>` for a server on this
machine, `remote: <url>` for any other host.

## Thinking

`"thinking": true` lets a model that thinks reason before it answers.
Its thinking goes to the log, never into `$output`.

`"thinking": false` is a request: the model's chat template decides, and
some ignore it. Thinking written in `<think>` at the start of an answer
is still kept out of `$output`, and `itero ws check` warns of a step
that asks a model that cannot turn it off.

With a `url`, `"thinking": false` asks for the lowest effort the model
takes, and `"thinking": true` sends nothing
([A model behind a URL](#a-model-behind-a-url)).

## While it answers

A running `#ai` step's progress is its token count (`[48 tokens]`), so a
long answer is not stopped as quiet; the answer streams from a `url` as
from itero-ai. The action's `limits` apply as to any action.
`itero ws check` checks its args before any run, and warns of a model
this machine does not have.

Every call is an `ai.answer` line in the workspace's log: where it was
sent, the model, the seconds it took and the whole response, with its
token counts (`usage`). `$output` is the answer alone.

A step naming a model the machine does not have fails, and the daemon
looks for models again at once: run it again a minute later. See
[Running and troubleshooting](/ai/running/).
