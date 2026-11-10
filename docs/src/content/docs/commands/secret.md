---
title: secret
description: Keeps a secret in the OS secret store, for every workspace or for one.
---

`itero secret` keeps a password, a token or an API key safe in your
system's secret store, so your workflows can use it as `$$NAME` without it
ever being written in a file. It is for every workspace on the machine;
[`itero ws secret`](#one-workspaces-secret) keeps one for a single
workspace.

```sh
itero secret <NAME>
itero secret <NAME> --file <path>
itero ws secret <NAME>
itero ws secret <NAME> --file <path>
```

Keeps a secret, for `$$NAME`, in the OS secret store: the macOS Keychain,
or the Secret Service on Linux. Without `--file` it asks for it, without
echo. With `--file` it reads it from the file, its last newline left out.
The value is never an argument, so `ps` and your shell history never show
it.

```
$ itero secret LM_STUDIO_TOKEN
LM_STUDIO_TOKEN: 
LM_STUDIO_TOKEN: kept in the OS secret store
```

A name is a letter or `_`, then letters, digits or `_`.

## One workspace's secret

`itero ws secret` keeps a secret for the workspace of this folder, or the
one named after `ws` (`itero ws quiet-river secret NAME`). Its `$$NAME` finds it before the machine's, so one
workspace can use another account without changing the others. It stays
with the workspace when the workspace is renamed.

```
$ itero ws secret IMAP_PASS
IMAP_PASS: 
IMAP_PASS: kept for workspace quiet-river in the OS secret store
```

`$$NAME` is looked up in the workspace's own secrets, then the machine's,
then the workspace's `.env`, then the daemon's environment. See
[Secrets and environment variables](/guides/secrets/).
