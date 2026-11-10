---
title: context
description: The itero servers this CLI talks to, each with its token and workspace.
---

`itero context` keeps the itero deployments you work with: your
development server, staging, production. A context is one deployment,
reached through its server: the server's address, a token to sign in
with, and the workspace you mean when you name none. It works like a
context in Kubernetes' kubeconfig.

```sh
itero context add <name> <url> [--ws <workspace>]
itero context use <name>
itero context list [--json]
itero context remove <name>
```

## add

Adds a context, after checking that its server answers. It asks for the
token without echo; in a script or CI, set `ITERO_TOKEN` instead. The
token is never an argument, so your shell history never shows it. An
empty token is none, for a server on `127.0.0.1`, which needs none.

```
$ itero context add prod https://itero.example.com --ws shop
prod's token (empty for none):
added prod: https://itero.example.com/, itero 0.38.0
```

It refuses a server it cannot reach, or one that refuses the token, and
saves nothing then. Adding a name already there replaces it. A name is
letters, digits, `-` and `_`.

## use, list, remove

`itero context use prod` makes it the one in use; `itero context use
local` goes back to this machine. `list` shows each, the one in use
marked, never a token:

```
$ itero context list
  local  this machine
* prod   https://itero.example.com/  shop
  dev    http://127.0.0.1:8787/
```

`local` is built in: this machine, as itero works without any context.
It cannot be added or removed.

## Commands over a context

With a context, a workspace command asks that server instead of this
machine, and says the same as it would here: the same text, the same
`--json`.

```sh
itero ws wf runs --context prod             # prod's runs, in its default workspace
itero ws billing check --context prod       # another workspace on prod
ITERO_CONTEXT=staging itero ws ev list      # for a script
```

The context is `--context`, else `ITERO_CONTEXT`, else the one in use
(`itero context use`), else this machine. The workspace is the one named
after `ws`, else the context's (a path is this machine's, so refused); with neither, the command names the server's workspaces
and asks which.

These go to the server: `ws info`, `ws check`, `ws which`, `ws emit`
(`--wait` up to 10 minutes), `ws wf list`, `ws wf runs`, `ws wf stop`,
`ws ac list`, `ws ac errors`, `ws cn list`, `ws ev list` and `ws ev
cancel`; and, about the server itself, `ws list` (its list) and `info`
(its daemon). The commands about this machine (`daemon`, `service`, `ws
add`, `ws remove`, `secret`, `emit` to every workspace, `ai`, `browser`,
`watch`, `webhook`) work here only: given `--context`, they say so; the
context in use does not change them.

## Another itero on the server

Over a context, the CLI and the server say their itero versions to each
other. When they differ, the command says so once, on stderr:

```
prod runs itero 0.39.0, this itero is 0.38.0
```

A server refuses a CLI older than it in the compatibility number (the
minor version below 1.0): upgrade this itero then. A newer CLI is served.

## Where they are kept

In `contexts.json` in itero's config folder: `$XDG_CONFIG_HOME/itero/`,
else `~/.config/itero/`, readable by you alone, as it holds the tokens.
