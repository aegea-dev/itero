# pomodoro

A pomodoro session: send `pomodoro.start` with how long it lasts, in
seconds, and a notification tells you when it is over.

```sh
itero ws emit pomodoro.start '{"duration": 1500}'
```

`workflows/pomodoro.json` gives the event's `duration` to its first step,
`actions/sleep`, which waits that long, then shows "Pomodoro session
finished" with `#notify`, and the run ends. A duration that is not a
whole number of seconds above 0 ends the run in error, saying why, and
nothing is shown.

While it waits, `itero ws wf runs` shows how long is left, as the step
says it each minute on stderr. The step holds a worker the whole time; a
daemon that stops meanwhile starts it again from the beginning.

Run `itero daemon` in this folder. Or open `pomodoro.ipynb`, a notebook that
installs itero and runs a session step by step, here or in
[Colab](https://colab.research.google.com/github/aegea-dev/itero/blob/main/examples/pomodoro/pomodoro.ipynb).

## Tests

`test/` runs two sessions on a throwaway copy of this folder: one of two
seconds, told when it is over, and durations it cannot wait for. It uses
`itero` from your `PATH` (or `$ITERO_BIN`), and needs Bun:

```sh
bun test
```
