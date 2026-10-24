---
title: secret
description: Keeps a secret in the OS secret store.
---

`itero secret` keeps a password, a token or an API key safe in your
system's secret store, so your workflows can use it as `$$NAME` without it
ever being written in a file.

```sh
itero secret <NAME>
itero secret <NAME> --file <path>
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

A name is a letter or `_`, then letters, digits or `_`. A `$$NAME` with
no secret of its name is the daemon's environment variable of that name.
See [Args and secrets](/guides/secrets/).
