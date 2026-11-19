# hello-world

The smallest workspace: send `greet.someone` with a name, and a
notification says hello. The same files as the
[Quickstart](https://docs.itero.sh/start/quickstart/).

```sh
itero ws emit greet.someone '{"name": "Ayşe"}'
```

`workflows/greet.json` gives the event's `name` to its first step,
`actions/hello`, a shell script that prints "Merhaba, Ayşe!". What it
prints is its result, which `#notify` shows as its text (`$output`), and
the run ends.

Run `itero ws daemon` in this folder.

## Tests

`test/` sends one greeting on a throwaway copy of this folder. It uses
`itero` from your `PATH` (or `$ITERO_BIN`), and needs Bun:

```sh
bun test
```
