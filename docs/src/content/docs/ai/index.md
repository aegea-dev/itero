---
title: itero-ai
description: "A local model for itero: what it is, how it fits, installing it."
---

**itero-ai** runs a language model on your own machine for itero. A
workflow asks it through the built-in [`#ai`](/ai/workflows/) step: to
sum up a mail, to sort it, to pull fields out of a text. Nothing leaves
the machine: no account, no key, no cloud.

itero ships no model. You bring one: any model file in the GGUF format,
the format llama.cpp and LM Studio use. itero advises
[Qwen3.5-4B](/ai/qwen3.5-4b/).

## How it fits

itero-ai is a program of its own, beside itero. You never start it:

- the daemon starts it when an `#ai` step needs it;
- it loads the model the step names, and answers;
- after five minutes with nothing asked, it exits and frees the model's
  memory; the next `#ai` step starts it again;
- when the daemon ends, it ends too.

A daemon in a workspace's folder has its own. The machine's daemon
([One itero per machine](/guides/machine/)) has one for every workspace
it runs, so a model is in memory once, however many workspaces use it.
It listens on a Unix socket only you can read, not on a network port,
and speaks OpenAI's Chat Completions API, so [other tools](/ai/api/) can
ask it too.

Without itero-ai, or without a model, nothing else in itero changes:
only `#ai` steps fail, saying why.

## Installing

```sh
brew install aegea-dev/tap/itero-ai
```

or its archive, `itero-ai-<version>-<target>.tar.gz`, from the
[release page](https://github.com/aegea-dev/itero/releases), with
`itero-ai` put beside `itero` or anywhere on your `PATH`.

## What it needs

- **macOS:** on Apple silicon the model runs on the GPU (Metal). Intel
  Macs have a build too, not measured.
- **Linux:** the model runs on the CPU, which is slower. The x86_64
  build does not use AVX2 yet.
- **Memory:** about the model file's size, plus room for its context.
  Qwen3.5-4B at 4 bits takes about 5.5 GB in all.

## A first answer

1. A model:

   ```sh
   itero ai models add unsloth/Qwen3.5-4B-GGUF
   ```

   (about 2.7 GB; a model you already have in LM Studio or Hugging
   Face's cache is found without downloading: `itero ai models list`).

2. A workspace with a workflow that asks it, and shows the answer as a
   notification, `workflows/sky.json`:

   ```sh
   mkdir ask && cd ask
   echo '{}' > itero.config.json
   mkdir workflows
   ```

   ```json
   {
     "on": [{ "event": "sky.asked" }],
     "steps": [
       { "#ai": { "args": { "model": "unsloth/Qwen3.5-4B-GGUF",
                            "text": "In one short sentence, under 15 words: why is the sky blue?" } } },
       { "#notify": { "args": { "title": "Why is the sky blue?", "text": "$output" } } }
     ]
   }
   ```

   `$output` is the model's answer. A notification shows about two
   lines; "under 15 words" keeps the answer to them (asked only for one
   sentence, Qwen3.5-4B answers in about 190 characters, cut in half).

3. Run it. With the [machine's daemon](/guides/machine/) running
   (`itero service install`), hand it the workspace:

   ```sh
   itero workspace add
   ```

   Without it, run a daemon in one terminal (`itero daemon`) and send
   the event from another. Either way:

   ```sh
   itero emit sky.asked
   ```

   The answer comes as a notification: "Sunlight scatters off air
   molecules, making the sky appear blue." No notification? `itero wf
   runs` shows the run and how it ended, and `itero wf runs <name>` each
   step's output and error. On macOS, notifications must be allowed for
   Itero (System Settings, Notifications).

The first answer takes a second or two more: itero-ai starts, and the
model is read into memory.

## In this section

- [Models](/ai/models/): where itero finds them, how they are named,
  choosing one.
- [Qwen3.5-4B](/ai/qwen3.5-4b/): the advised model and its settings.
- [`#ai` in a workflow](/ai/workflows/): the step, its answers and args.
- [The API](/ai/api/): asking itero-ai from other tools.
- [Running and troubleshooting](/ai/running/): its life, its events,
  common errors.
- [Testing](/ai/testing/): testing workflows with `#ai` without a model.

The commands are under [`itero ai`](/commands/ai/).
