# notify-api

Notifications for an app on this machine, over HTTP: the app asks itero to
show one, and itero posts the answer back when it is clicked. macOS.

The app sends an `app.notify` event to the daemon's API, with what to show
as its data, and where to post the answer as `reply_url`:

```sh
curl -s -X POST http://127.0.0.1:8800/events -H 'content-type: application/json' -d '{
  "type": "app.notify",
  "data": {"title": "Deploy?", "text": "3 changes", "buttons": ["Deploy", "Cancel"],
           "reply_url": "http://localhost:3000/notified"}
}'
```

`workflows/app-notify.json` shows it with `#notify`, giving it the
event's `title`, `text`, `subtitle`, `sound`, `url`, `buttons`, `wait` and
`data` as its args (`$event.title` and so on; one the app leaves out is
left out). With `buttons` (or `"wait": true`) it waits for a click, which
is its result, `$output`; then its `success` step, `actions/post-answer`,
posts to `reply_url` what the app asked and the answer:

```json
{ "asked": { "title": "Deploy?", "text": "3 changes", "buttons": ["Deploy", "Cancel"],
             "reply_url": "http://localhost:3000/notified" },
  "answer": "Deploy", "notification": "01a1…" }
```

`answer` is the button's label, `open` for a click on the notification
itself, or `dismissed` when it is closed without one. Anything else the
app puts in the event comes back in `asked`, so it knows which request
was answered. A notification nobody answers ends in error at the
workflow's `timeout` (an hour), and its `error` step posts that too, with
`why`. Without `buttons` or `wait`, the run ends as soon as it is shown,
and is posted then.

Run `itero daemon` in this folder.

## Tests

`test/` has one test: a request is shown, its click is answered, and the
answer reaches a server standing in for the app. It runs on a throwaway
copy of this folder, with `itero` from your `PATH` (or `$ITERO_BIN`), and
needs Bun:

```sh
bun test
```
