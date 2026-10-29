---
title: ai
description: The local models #ai steps use, and one model file's header.
---

`itero ai` looks after the models [`#ai`](/ai/workflows/) steps use; see [itero-ai](/ai/).

```sh
itero ai models list [--all] [--json]
itero ai models add <org>/<repo>[:<QUANT>] [--yes]
itero ai models remove <org>/<repo>[:<QUANT>] [--delete]
itero ai inspect <file.gguf> [--json]
```

## `models list`

The models on this machine, looked for again first: in LM Studio's
folder, Hugging Face's cache (`$HF_HUB_CACHE`, else `$HF_HOME/hub`, else
`~/.cache/huggingface/hub`) and what `add` downloaded. Each with its
parameters, architecture, quantization, context, size and licence, and
whether it chats, runs here and has its thinking split from its answer.

A model is named by where it comes from, `<org>/<repo>:<QUANT>`, as
Hugging Face names it: that name goes in a step's `model`. `--all` shows
the removed ones too. The list is kept in `~/.cache/itero/models.json`;
the daemon's `models` workflow looks again when it starts and every
hour (the machine's daemon looks once for all its workspaces).

## `models add`

Downloads a model from Hugging Face into `~/.cache/itero/models/`. Which
file: the quantization named, else the repo's only model, else its
`Q4_K_M`, else an error naming them. It shows the file, its size and its
licence first (a licence other than MIT or Apache-2.0 is warned of),
asks before downloading unless `--yes`, and refuses when the disk has
too little room. A download that breaks resumes when run again, and is
checked against the SHA-256 Hugging Face publishes.

A model that asks you to accept its terms needs a Hugging Face token:
`itero secret HF_TOKEN`. `$HF_ENDPOINT` points at a mirror.

## `models remove`

Takes a model out of use: steps naming it fail, saying so, and its file
stays. `add` takes it back. `--delete` deletes the file too, only one
`add` downloaded; another app's is refused, naming where it is.

## `inspect`

One GGUF file's header, without its weights: what it is, and whether
this build of llama.cpp can run it.
