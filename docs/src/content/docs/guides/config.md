---
title: Writing itero.config.json
description: What goes in a workspace's config and what does not, your own keys, your editor's help, and what a mistake looks like.
---

`itero.config.json` makes a folder a workspace, and says what the
workspace is and how it runs. Every key has a default, so `{}` is a whole
workspace; you add keys as you need them. Every key is in the
[reference](/reference/itero-config/).

## What it is

Give the workspace a name and a few words about it:

```json
{
  "$schema": "https://docs.itero.sh/schema/itero.config.json",
  "name": "home",
  "version": "0.1.0",
  "description": "Mail and a beacon on this machine",
  "tags": ["home", "mail"]
}
```

A name is lowercase letters, digits and hyphens (`home`, `shop-orders`),
so it can go in an address. `itero ws info` shows them all.

`$schema` is for your editor: VS Code, JetBrains IDEs and other editors
that read JSON Schema then suggest every key as you type, say what each
is, and underline a mistake before itero sees it. What the editor
underlines is what itero refuses: both read the same
[schema](/reference/itero-config/#the-schema). JetBrains IDEs and other
editors that use [SchemaStore](https://www.schemastore.org)'s catalog find
it by the file's name, without `$schema`; VS Code needs the line.

## How it runs

Most workspaces need none of this. Set `port` when an app needs a fixed
address for the API, `folders` when your workflows or actions live
elsewhere, and `limits` when the defaults do not fit:

```json
{
  "port": 8787,
  "folders": { "workflows": ["workflows", "shared/workflows"] },
  "limits": { "runs": 50, "keep": "7d", "quiet": "30m" }
}
```

What each limit does, and its default:
[`limits`](/reference/itero-config/#limits).

## An entry for each workflow, action and connector

An entry describes a workflow, an action or a connector that is in its
folder, by its name; it never makes one. A workflow's entry can also turn
it off, or on only between two dates:

```json
{
  "workflow": {
    "sites": { "description": "Each site, every minute", "tags": ["beacon"] },
    "sale": { "from": "2026-11-27", "until": "2026-11-30" },
    "keep": false
  },
  "action": { "imap-check": { "description": "New mail on the server" } },
  "connector": { "mail.received": true }
}
```

`true` is an entry with nothing to say, `false` turns it off (`keep` is
one of [itero's own workflows](/guides/maintenance/#built-in-keep-logs-and-models):
off, nothing forgets ended runs). An entry
that names nothing is an error: a name with a typo never leaves a workflow
running that you meant to turn off.

## Your own keys: `metadata`

Who looks after the workspace, a region, a link to its runbook: put them in
`metadata`, at the top or in an entry. Itero keeps them, shows them, and
never acts on them:

```json
{
  "metadata": { "owner": "ayşe", "region": "eu" },
  "workflow": { "report": { "metadata": { "dashboard": "https://example.com/d/42" } } }
}
```

A workflow reads the workspace's as `$config.metadata`:
`{ "#report": { "args": { "region": "$config.metadata.region" } } }`.

## What does not go in it

The config is a manifest, like `package.json`: it says what the workspace
is, never the values a workflow works on. Those come from elsewhere:

| A value | Goes |
|---|---|
| what happened (an order, a mail) | in the event |
| what a step works with (a URL, a limit) | in the step's `args`, in its workflow |
| a password, a token, a key | in `.env` or the secret store, read as `$$NAME` ([Secrets](/guides/secrets/)) |

A key from before, like a workflow's `settings` or an action's `args`,
is an error that says where it goes now.

## When it is wrong

Every command reads the config first. A key itero does not know, or a
value of the wrong form, is an error, and the command says every one at
once, by its place in the file, then does nothing else:

```
$ itero ws check
itero.config.json
  error: limits.keep: a duration like 30m or 1d, not "1x"
  error: workflow.greet: "of" is not a key; did you mean "off"?
  error: "prot" is not a key; did you mean "port"?
  error: "owner" is not a key; your own keys go in "metadata"
4 errors, 0 warnings
```

`itero ws daemon` does not start, and the machine's
[`itero daemon`](/commands/daemon/#problems-in-your-files) runs nothing in
the workspace, saying so, and starts it once the file is put right.
