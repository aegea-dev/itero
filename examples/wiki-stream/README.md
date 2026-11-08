# wiki-stream

Every edit to a Wikipedia, live, as it happens, from Wikimedia's
[EventStreams](https://stream.wikimedia.org/?doc): no key, no sign-up,
some five edits a second. Two workflows watch the stream:

- `pulse`: every 10 seconds, what they brought, in a line: "41 edits, 14
  by bots; most on enwiki 12, frwiki 4, astwiki 3";
- `busy-page`: a page edited by people 3 times within 5 minutes, told at
  most once in 10 minutes per page.

```sh
itero daemon
```

It needs `curl` and `jq`.

1. The connector, `connectors/wiki.edited`, reads the stream with `curl`
   and turns each edit or new page on a `*.wikipedia.org` into an event
   with `jq`: its id, time, wiki, title, user, whether a bot made it, how
   many bytes it changed, and its address.
2. `pulse` keeps the edits of each 10 seconds for one run (`batch`), whose
   `$event.events` `actions/summarize` counts.
3. `busy-page` counts edits by people per page (`at_least`, `per` the
   page's address), and its notification has a window of its own
   (`at_most`), so a page is told once.
4. Both drop an edit they have seen (`unique` on its id).

Stop the daemon and start it again: the connector notes the time of the
last edit it told (`.itero/wiki.edited.since`), and asks the stream for
what came since. The first summary after a restart has what came while
it was down, and an edit sent twice is dropped by `unique`.

`wiki-stream.ipynb` runs it step by step in a notebook, in
[Colab](https://colab.research.google.com/github/aegea-dev/itero/blob/main/examples/wiki-stream/wiki-stream.ipynb):
nothing to install. With no desktop there, notifications go to a file.

Wikimedia asks clients for a User-Agent that says who they are: the
connector's names this example.
