---
title: Writing your first workflow
description: A workflow that checks a website and tells you when it is down, one step at a time.
---

You will write a workflow that checks a website
every five minutes and shows a notification when it is down. It takes one
action and a few lines of JSON.

## 1. A workspace

```sh
mkdir site-check && cd site-check
echo '{}' > itero.config.json
mkdir workflows actions
```

## 2. An action

An action is a small program in `actions/`. This one, `actions/ping`,
fetches a page and says whether it answered:

```sh
#!/bin/sh
if curl -fsS -o /dev/null --max-time 10 "$url"; then echo true; else echo false; fi
```

```sh
chmod +x actions/ping
```

It gets its args as environment variables (`$url`). What it prints is
its result, `true` or `false`: the next step has it as `$output`.

## 3. The workflow

`workflows/site.json`:

```json
{
  "on": [{ "schedule": "*/5 * * * *" }, { "event": "site.check" }],
  "steps": [
    { "#ping": { "args": { "url": "https://example.com" } } },
    { "#notify": { "on": [{ "$output": false }],
                   "args": { "title": "Site is down", "text": "https://example.com does not answer" } } }
  ]
}
```

- `on`: when it runs. Every five minutes, or when a `site.check` event
  comes, so you can try it now.
- `#ping`: run the action, with `url` as its arg. An action gets its
  args and nothing else.
- `#notify`: show a notification (it is built in), but only when its `on`
  holds: `#ping` printed `false`.

## The expressions

| Write | Means |
|---|---|
| `"$event.url"` | the `url` of the event that started the run |
| `"$output"` | the step before's result |
| `"$steps.ping"` | `#ping`'s result, by its name, in any later step |
| `"$event.url is down"` | a text with the event's `url` in it |
| `{ "$output": false }` | holds when the step before printed exactly `false` |
| `{ "has": "$event.url" }` | holds when the event has a `url` |
| `"$$TOKEN"` | the secret `TOKEN`, or a value in `.env` ([Args and secrets](/guides/secrets/)) |

A condition is a field and the value it must have; a step whose `on` does
not hold is skipped.

## 4. Run it

```sh
itero check     # says what is wrong, if anything
itero daemon    # leave it running
```

In another terminal, in the same folder:

```sh
itero emit site.check
itero wf runs           # the run, with its name
itero wf runs <name>    # what each step did
```

```
hidden-beacon  01a17240-01ec-745c-889b-a153818de0c8  site  done
  2026-10-25T08:49:19Z  started  {}
  2026-10-25T08:49:19Z  #ping  done  true
  2026-10-25T08:49:19Z  ended done
```

The site is up, so `#notify` did not run. Change the url to
`http://localhost:9`, where nothing answers, and send `site.check` again:
a notification says the site is down.

## Next

- [Workflows](/guides/workflows/): more examples, steps side by side,
  workflows as steps, what runs at the end.
- [Actions](/guides/actions/): what an action gets and how it answers.
