---
title: Models
description: Where itero finds GGUF models, how they are named, and choosing one.
---

A model is one GGUF file: the weights and what the model needs to run,
such as its chat template. itero works with the files you already have,
and downloads others from Hugging Face.

## Where itero finds them

- **LM Studio's folder:** `~/.lmstudio/models`;
- **Hugging Face's cache:** `$HF_HUB_CACHE`, else `$HF_HOME/hub`, else
  `~/.cache/huggingface/hub` (where llama.cpp's `-hf` and Hugging Face's
  tools download to);
- **itero's own:** `~/.cache/itero/models`, where
  `itero ai models add` downloads to (`$XDG_CACHE_HOME/itero/models`
  when that is set).

A file has to be two folders down, as `<org>/<repo>/<file>.gguf`: that
is where its name comes from. A file in a folder of your own is not
found; to use it, put it in `~/.cache/itero/models/<org>/<repo>/`, with
names of your choice.

Vision projectors (`mmproj-*.gguf`) are not models and are left out. A
model in parts (`...-00001-of-00003.gguf`) is its first part.

## Names

A model is named by where it comes from, as Hugging Face and llama.cpp
name it:

```
<org>/<repo>:<QUANT>
unsloth/Qwen3.5-4B-GGUF:Q4_K_M
```

That name goes in an `#ai` step's `model`. Without `:<QUANT>`, the
name means the repo's only file, else its `Q4_K_M`, else it is an
error that names the ones there.

The quantization comes from the file's name (`Qwen3.5-4B-Q4_K_M.gguf`);
the file's header is read only when the name has none. Unsloth's
Dynamic files keep their `UD-` prefix: `Qwen3.5-4B-UD-Q4_K_XL.gguf` is
`unsloth/Qwen3.5-4B-GGUF:UD-Q4_K_XL`.

## Choosing a quantization

A quantization stores the weights in fewer bits: a smaller file, less
memory, faster answers, and a little further from the original model.

- **`Q4_K_M`:** 4 bits, mostly; a good balance, and what itero picks
  when a repo has several. Qwen3.5-4B's is 2.74 GB.
- **`UD-Q4_K_XL`:** Unsloth's Dynamic 4 bits, which keeps the layers that
  matter most in more bits. Qwen3.5-4B's is 2.91 GB.
- **`Q8_0`:** 8 bits, close to the original.
- **`F16`, `BF16`:** the original weights, the largest.

Start with `Q4_K_M`; try a larger one when the answers are not good
enough, and measure on your own workflows.

## The commands

See [`itero ai`](/commands/ai/) for every flag.

```sh
itero ai models list
itero ai models add unsloth/Qwen3.5-4B-GGUF
itero ai models remove unsloth/Qwen3.5-4B-GGUF:Q4_K_M
```

- **`list`** looks again, then shows each model with its parameters,
  architecture, quantization, context, size and licence, and whether it
  chats, runs with this itero-ai, and thinks.
- **`add`** downloads from Hugging Face: it shows the file, its size
  and licence first, resumes a broken download, and checks the file
  against Hugging Face's SHA-256. A licence other than MIT or Apache-2.0
  is warned of.
- **`remove`** takes a model out of use and keeps its file; `--delete`
  deletes the file too, only one `add` downloaded.

A model that asks you to accept its terms on Hugging Face needs your
token: `itero secret HF_TOKEN`.

## The list

The models are kept in `~/.cache/itero/models.json`, read by the daemon
and by itero-ai. The daemon's built-in `models` workflow updates it when
the daemon starts, when a step names a model it does not have, and every
hour; it reads only new or changed files. See
[The daemon's own workflows](/guides/maintenance/), and to turn it off:

```json
{ "workflow": { "models": { "off": true } } }
```

A step that names a model the list does not have yet fails, and the
`models` workflow looks again at once: run it again a minute later.
