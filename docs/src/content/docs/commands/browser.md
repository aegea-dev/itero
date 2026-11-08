---
title: browser watch
description: For connectors - keeps a page open and prints a line each time something happens on it.
---

`itero browser watch` is for connectors: it keeps a page open in the
daemon's Chrome and prints a JSON line, one event, each time something
happens on it. [The browser](/guides/browser/#watching-a-page) has the
whole story.

```sh
itero browser watch <url> [--changed <target>]... [--appeared <target>]... [--navigated] [--responses <text>]... [--reload <seconds>] [--headless]
```

| Option | A line when |
|---|---|
| `--changed <target>` | the target's text changed, after a second of quiet, so a burst is one line: `text`, `previous` |
| `--appeared <target>` | the target came onto the page: `text` |
| `--navigated` | the page went to another address, reloaded, or changed in place |
| `--responses <text>` | the page received JSON from an address containing the text: `json`, `response_url` |
| `--reload <seconds>` | (no line of its own) loads the page again that often, for a page that changes only when loaded; what changed is told as `--changed` and `--appeared` say, and its own reloads are not `--navigated` |
| `--headless` | (no line) in a headless Chrome, not a window |

Every line has `what` (`changed`, `appeared`, `navigated` or
`response`), the page's `url` and `title`, `target` for the first two,
and `tab`, the window's address, which a `#browser` step's `tab` takes to
act on the same page. A target is a name, as a step's is, or
`{"css": "..."}`.

It runs only as a connector of a daemon that serves its API: the daemon
gives it its window (`ITERO_BROWSER_API`), and closes it when the
connector ends, however it ends (stopped, killed or crashed), or when the
daemon ends.

```sh
#!/bin/sh
# connectors/page.list.changed
exec itero browser watch "$URL" --changed "$TARGET"
```
