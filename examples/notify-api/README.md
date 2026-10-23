# notify-api

Notifications for an app on this machine, over HTTP: the app asks itero to
show one, and itero posts the answer back when it is clicked. macOS.

The app sends an `app.notify` message to the daemon's API, with what to
show as its data and where to post the answer as `reply`:

```sh
curl -s -X POST http://127.0.0.1:8800/events -H 'content-type: application/json' -d '{
  "type": "app.notify",
  "data": {"title": "Deploy?", "text": "3 changes", "buttons": ["Deploy", "Cancel"]},
  "reply": "http://localhost:3000/notified"
}'
```

`workflows/app-notify.json` is one step, `#notify`, which takes `title`,
`text`, `subtitle`, `sound`, `url`, `buttons`, `wait` and `data` from the
message. With `buttons` (or `"wait": true`) it waits for a click; the run
then ends, and is posted to the `reply` URL:

```json
{ "message": "01a1…", "run": "01a1…", "name": "misty-lake", "workflow": "app-notify",
  "status": "done",
  "state": { "title": "Deploy?", "text": "3 changes", "buttons": ["Deploy", "Cancel"],
             "notification": "01a1…", "answer": "Deploy", "workflow": "app-notify" } }
```

`answer` is the button's label, `open` for a click on the notification
itself, or `dismissed` when it is closed without one. Anything else the
app puts in `data` comes back in `state`, so it knows which request was
answered. A notification nobody answers ends at the workflow's `timeout`
(an hour), and that is posted too. Without `buttons` or `wait`, the run
ends as soon as it is shown, and is posted then.

Run `itero daemon` in this folder.

## Tests

`test/` has one test: a request is shown, its click is answered, and the
answer reaches a server standing in for the app. It runs on a throwaway
copy of this folder, with `itero` from your `PATH` (or `$ITERO_BIN`), and
needs Bun:

```sh
bun test
```
