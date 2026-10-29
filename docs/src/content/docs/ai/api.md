---
title: The API
description: OpenAI's Chat Completions on itero-ai's socket, for other tools.
---

itero-ai speaks OpenAI's Chat Completions API, over HTTP on a Unix
socket. Most tools and libraries that talk to OpenAI can talk to it:
point them at the socket, and name a model as [Models](/ai/models/)
does.

It answers only on its socket, never on a network port. The daemon puts
the socket in a folder only you can read.

## The socket

The daemon starts itero-ai on a socket of its own, and tells where in
the `itero.ai.started` event's `socket` field: one socket for each
workspace's own daemon, one for the machine's daemon, named after
its state folder. To run one
yourself, on a socket of your own, see
[Running and troubleshooting](/ai/running/).

The examples below use `curl`:

```sh
curl --unix-socket "$SOCKET" http://localhost/health
```

The host name in the URL is not used; `localhost` is fine.

## `POST /v1/chat/completions`

```sh
curl --unix-socket "$SOCKET" http://localhost/v1/chat/completions \
  -H 'content-type: application/json' \
  -d '{ "model": "unsloth/Qwen3.5-4B-GGUF",
        "messages": [{ "role": "user", "content": "Name one colour of the sky. One word." }],
        "temperature": 0.7, "top_p": 0.8, "top_k": 20, "min_p": 0, "presence_penalty": 1.5 }'
```

```json
{
  "id": "chatcmpl-18e30ef7b07770b0",
  "object": "chat.completion",
  "created": 1793293533,
  "model": "unsloth/Qwen3.5-4B-GGUF",
  "choices": [{ "index": 0, "finish_reason": "stop",
                "message": { "role": "assistant", "content": "Blue" } }],
  "usage": { "prompt_tokens": 22, "completion_tokens": 1, "total_tokens": 23 }
}
```

The fields it takes:

| Field | Is |
|---|---|
| `model` | `<org>/<repo>`, with `:<QUANT>` or without; needed |
| `messages` | a list of `{ "role", "content" }`: roles `system`, `user`, `assistant`; content is text; needed |
| `max_completion_tokens`, `max_tokens` | the longest answer, in tokens |
| `temperature`, `top_p`, `seed`, `presence_penalty`, `frequency_penalty`, `stop` | as OpenAI's |
| `top_k`, `min_p` | not OpenAI's, but llama.cpp's and vLLM's |
| `chat_template_kwargs` | `{ "enable_thinking": true }` lets a model that thinks think first |
| `response_format` | `text`, `json_object`, or `json_schema` with a schema the answer must follow |
| `stream` | `true` for the answer in pieces |
| `stream_options`, `n` | taken; `n` can only be 1 |

**Anything else is refused, by name.** itero-ai never ignores a field it
does not honour, so a tool cannot believe it got what it asked for:

```json
{ "error": { "message": "tools is not something itero-ai does",
             "type": "invalid_request_error", "param": "tools" } }
```

with status 400. Images and audio in `content` are refused the same
way.

### An answer in a form

With `json_schema`, the schema becomes a grammar: the model can write
nothing else. Tell it the form in the message too; a small model held by
the grammar alone answers poorly.

```json
"messages": [{ "role": "user", "content": "Invoice: 1250 TL due 2026-11-15. Answer as a JSON object: {\"amount\": <number>, \"due\": \"<YYYY-MM-DD>\"}." }],
"response_format": { "type": "json_schema", "json_schema": { "name": "invoice",
  "schema": { "type": "object", "required": ["amount", "due"],
              "properties": { "amount": { "type": "number" },
                              "due": { "type": "string", "format": "date" } } } } }
```

gives the content `{"amount": 1250, "due": "2026-11-15"}`.

### Thinking

With `"chat_template_kwargs": { "enable_thinking": true }`, a model that
thinks reasons first. Its reasoning comes apart from its answer, in
`reasoning_content`, as llama.cpp's server gives it:

```json
"message": { "role": "assistant", "content": "\n\nNo",
             "reasoning_content": "Thinking Process:\n\n1.  **Analyze the Request:** ..." }
```

### Streaming

With `"stream": true`, the answer comes as server-sent events, one
`chat.completion.chunk` per piece. The last chunk has the
`finish_reason` and the `usage`; then `data: [DONE]`.

```
data: {"choices":[{"delta":{"content":"Mer"},"finish_reason":null,"index":0}],"object":"chat.completion.chunk",...}
data: {"choices":[{"delta":{"content":"hab"},"finish_reason":null,"index":0}],"object":"chat.completion.chunk",...}
data: {"choices":[{"delta":{"content":"a"},"finish_reason":null,"index":0}],"object":"chat.completion.chunk",...}
data: {"choices":[{"delta":{},"finish_reason":"stop","index":0}],"object":"chat.completion.chunk","usage":{...}}
data: [DONE]
```

Thinking comes in `delta.reasoning_content`, the answer in
`delta.content`. A model the machine does not have is an error status,
before any chunk.

### Errors

Errors have OpenAI's shape, `{ "error": { "message", "type", "param" } }`:

- **400:** a request it cannot answer as asked, `param` naming the field;
- **404:** no such model on this machine, `param` is `model`;
- **500:** the model failed.

## `GET /v1/models`

The models on this machine, from itero's list (removed ones left out),
each with OpenAI's `id`, `object` and `owned_by`, and itero's own
fields: `quant`, `params`, `architecture`, `context`, `bytes`,
`licence`, `chat`, `runs`, `thinking`.

## `GET /health`

```json
{ "loaded": "unsloth/Qwen3.5-4B-GGUF:Q4_K_M", "waiting": 0, "idle": 0 }
```

The model loaded (`null` for none), how many requests wait, and seconds
since the last one. Asking `/health` does not count as a request: it
does not keep itero-ai from exiting when idle.

## `POST /itero/inspect`

One GGUF file's header, without its weights:

```sh
curl --unix-socket "$SOCKET" http://localhost/itero/inspect \
  -H 'content-type: application/json' \
  -d '{ "path": "/path/to/Qwen3.5-4B-Q4_K_M.gguf" }'
```

```json
{ "architecture": "qwen35", "basename": "Qwen3.5-4B", "bytes": 2740937888, "chat": true,
  "context": 262144, "licence": "apache-2.0", "maker": null, "params": "4B",
  "quant": "Q4_K_M", "runs": true, "thinking": true }
```

`itero ai inspect` gives the same.

## One at a time

itero-ai holds one model in memory and answers one request at a time;
others wait their turn. A request for another model unloads the one
loaded first.
