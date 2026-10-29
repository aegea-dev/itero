# Plan 20: an itero-ai section in the docs

`#ai`, itero-ai and the models they use are a subject of their own: one
guide page (`guides/ai.md`) no longer holds it. This plan gives them a
group of their own in the docs sidebar. Docs only: no behaviour changes.

## Before it

The fix that keeps Unsloth's `UD-` prefix in model names
(`unsloth/Qwen3.5-4B-GGUF:UD-Q4_K_XL`, today read as `Q4_K_XL`) lands
first, in its own PR. The Models page then describes names as they are.

## The group

A sidebar group **itero-ai**, between Guides and Reference. The pages,
their addresses and what each holds:

| Page | Address | Holds |
|---|---|---|
| Overview | `/ai/` | What itero-ai is and why it runs locally; how it fits (the daemon starts it on demand, the Unix socket, exiting when idle); installing (Homebrew, the release archive); what it needs (Metal on macOS, the CPU on Linux, memory for the model) |
| Models | `/ai/models/` | Where itero finds models (LM Studio's folder, Hugging Face's cache, `~/.cache/itero/models`); how they are named (`<org>/<repo>:<QUANT>`, the `UD-` files); choosing a quantization, in plain words; `itero ai models list`, `add`, `remove`; models that need a token (`$$HF_TOKEN`); the list in `~/.cache/itero/models.json`; the built-in `models` workflow |
| Qwen3.5-4B | `/ai/qwen3.5-4b/` | The advised model and why (size, speed, Apache-2.0); its files; its makers' sampling by mode, thinking on and off; its context in itero; what we measured (say the answer's form in the text too; at temperature 0 it leans to the first label) |
| `#ai` in a workflow | `/ai/workflows/` | Today's `guides/ai.md`, its `#ai` part: the three kinds of answer, the args, examples, progress, `itero check` |
| The API | `/ai/api/` | For other tools: OpenAI's Chat Completions on the socket; the fields it takes and the ones it refuses by name; its extensions (`top_k`, `min_p`, `chat_template_kwargs.enable_thinking`, `reasoning_content`); streaming; errors; `/v1/models`, `/health`, `/itero/inspect`; `curl --unix-socket` examples |
| Running and troubleshooting | `/ai/running/` | Starting, idling, failing and the doubling pause; the `itero.ai.*` events; its environment (`ITERO_AI_SOCKET`, `ITERO_AI_IDLE`, `ITERO_AI_INDEX`); common errors: not installed, a model missing, a socket path over 104 bytes, a slow first load |
| Testing | `/ai/testing/` | Workflows with `#ai` and no model: `ITERO_AI_SOCKET` and a small stand-in server, as in the `ai-mail` example |

`itero ai` stays under Commands, as every command does, and links to
the group; the group links back to it rather than repeating it.

## Moves and links

- `guides/ai.md` is split across the group; its address `/guides/ai/`
  redirects to `/ai/workflows/` (`redirects` in `astro.config.mjs`).
- Every link to `/guides/ai/` in the docs, the README and the examples
  points at the right new page.
- The Guides entry "A local model in a step" goes; the group replaces it.
- A page's address with a dot (`qwen3.5-4b`) is set in its frontmatter
  (`slug`) if the default drops the dot; the build shows which.

## Writing

- Every fact is read from the code, the tests or a run, not from
  memory; what could not be checked on Linux is said.
- Plain words for newcomers, one idea a sentence, examples a reader can
  run. Symbols only after their word, in headings and the sidebar.
- itero's own words (args, step, event, workflow, action); no new term
  without the owner's yes.
- No em dash.

## Done when

- `yarn test` and `yarn build` pass in `docs/`, with no broken link.
- Each page is read once in the built site (the preview Cloudflare builds
  for the branch).
- One PR, docs only.
