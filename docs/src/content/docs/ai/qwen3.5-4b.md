---
title: Qwen3.5-4B
slug: ai/qwen3.5-4b
description: "The model itero advises: its files, its sampling by mode, what we measured."
---

itero advises **Qwen3.5-4B**, by the Qwen team, for `#ai` steps:

- **small:** 2.74 GB as `Q4_K_M`, about 5.5 GB of memory in all;
- **fast on a laptop:** about 40 tokens a second on an M1 Max;
- **open:** Apache-2.0, for any use;
- **thinks when asked:** it can reason before it answers, or answer at
  once.

Its context is 262,144 tokens; itero gives each answer 16,384 for now,
the prompt included.

## Its files

Unsloth's GGUF files are the ones itero is tested with:

```sh
itero ai models add unsloth/Qwen3.5-4B-GGUF
itero ai models add unsloth/Qwen3.5-4B-GGUF:UD-Q4_K_XL
```

The first is `Q4_K_M`; the second Unsloth's Dynamic 4 bits (2.91 GB).
See [Models](/ai/models/) for choosing one.

Unsloth fixed Qwen3.5's chat template on March 5, 2026. A file
downloaded before then has the old one: download it again.

## Sampling by mode

Qwen's makers advise different sampling for each kind of work. Without
any, a step gets llama.cpp's defaults, which are not these. Thinking is
off unless a step asks for it with `"thinking": true`.

**Without thinking, general work** (sorting, summing up, fields):

```json
"temperature": 0.7, "top_p": 0.8, "top_k": 20, "min_p": 0, "presence_penalty": 1.5
```

**Without thinking, reasoning work:**

```json
"temperature": 1.0, "top_p": 0.95, "top_k": 20, "min_p": 0, "presence_penalty": 1.5
```

**With thinking, general work:**

```json
"thinking": true,
"temperature": 1.0, "top_p": 0.95, "top_k": 20, "min_p": 0, "presence_penalty": 1.5
```

**With thinking, precise work** (code):

```json
"thinking": true,
"temperature": 0.6, "top_p": 0.95, "top_k": 20, "min_p": 0, "presence_penalty": 0
```

These go in the step's `args`, beside `model` and `text`. Source:
[Unsloth's Qwen3.5 guide](https://unsloth.ai/docs/models/qwen3.5), as of
October 2026.

## What we measured

itero's own tests run Qwen3.5-4B (`make test-ai`). What they taught:

- **Say the form.** Held to a form by a grammar alone, it answers
  poorly; told the form in words as well, it answers well. `#ai` adds
  that sentence to `text` for `choose` and `fields`.
- **No temperature 0 for sorting.** At temperature 0, a mail it finds
  borderline goes to the label listed first: the same mail was `urgent`
  with `["urgent", "later", "spam"]` and `later` with `later` first.
  Use its sampling above, and list the labels in the order you would
  pick when unsure.
- **Thinking takes time.** With thinking on, an answer can take many
  times as many tokens. Turn it on for steps that need reasoning, not
  for sorting.
