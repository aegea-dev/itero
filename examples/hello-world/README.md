# hello-world

The smallest workspace: send `greet.someone` with a name, and a
notification says hello. The same files as the
[Quickstart](https://docs.itero.sh/start/quickstart/).

```sh
itero ws emit greet.someone '{"name": "world"}'
```

`workflows/greet.json` gives the event's `name` to its first step,
`actions/hello`, a shell script that prints "Hello, world!". What it
prints is its result, which `#notify` shows as its text (`$output`), and
the run ends.

Run `itero ws daemon` in this folder.
