# page-watch

A notification each time the newest post on Hacker News changes, seen on
the page itself in Chrome, as a person would see it.

```sh
itero daemon
```

1. The connector, `connectors/page.changed`, is one line:
   `itero browser watch` keeps the page open in a headless Chrome the
   daemon starts, loads it again each minute (`--reload 60`: Hacker News
   changes only when loaded), and prints a line each time the target's
   text changes. A page that changes by itself (a chat, a live price)
   needs no `--reload`.
2. The workflow gives it the page and the target (the first post's
   title, by CSS), and shows each change as a notification; a click on
   it opens the page.

Another page: change `URL` and `TARGET` in `workflows/newest.json`. A
target is a name as a person sees it (a list's label, a heading) or
`{"css": "..."}`. For a site that checks for a person, leave out
`--headless`: Chrome then opens a window you can pass the check in once.

It needs Chrome.
