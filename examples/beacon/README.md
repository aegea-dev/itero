# beacon

A beacon: health checks on a schedule, and a notification only when
something changes. Every minute each site the sites workflow names
is asked; every 10 minutes the disk's free space is checked.
When a site goes down, comes back, or the disk runs low, you hear about
it once.

Run `itero daemon` in this folder and leave it running. The first check
tells you what it watches; after that, only changes. Your own sites:
the `sites` arg of `#check-sites` in `workflows/sites.json` (the disk's
`min_free` is in `workflows/disk.json`). Your own checks: an action that
prints {"<what>": "<how it is>"}, its result, followed by #changed (given
it as `"now": "$output"`) and #notify (it shows `$output.text`). The
actions need curl, jq, df and awk.

## Tests

`test/` has three tests: the first check tells what it watches, nothing is told
when nothing changed, and a new site is told. They use a port nothing
answers on, so they need no network. Each test runs on a throwaway copy of this folder,
with `itero` from your `PATH` (or `$ITERO_BIN`). They need Bun,
and nothing else installed:

```sh
bun test
```
