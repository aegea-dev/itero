# ai-mail

Sorts each mail with a local model, `urgent`, `later` or `spam`, and
shows a notification for the urgent ones. The model runs on your own
machine through itero-ai; nothing leaves it.

```sh
brew install aegea-dev/tap/itero-ai
itero ai models add unsloth/Qwen3.5-4B-GGUF
itero daemon
itero emit mail.received '{"from": "monitoring", "subject": "Database down", "body": "Customers cannot pay."}'
```

A mail connector would send `mail.received` on its own; here it is sent
by hand. The sampling in `workflows/sort.json` is what Qwen3.5-4B's
makers advise without thinking.

Its tests need no model: `cd test && ITERO_BIN=$PWD/../../../target/debug/itero bun test`.
A small server on a Unix socket stands in for itero-ai (`test/fake-ai.js`,
in its own process, through `ITERO_AI_SOCKET`).
