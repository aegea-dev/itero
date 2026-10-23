# Plan 10: clicks on notifications

A click on a notification itero showed becomes a message,
`itero.notification.clicked`, in the workspace that showed it. A workflow
can start on it, and a `#notify` step can wait for it: a notification with
buttons asks a person, and the run goes on with their answer. macOS only;
Linux keeps today's notifications.

## Spec

```json
{ "steps": [
    "#plan",
    { "#notify": { "title": "Deploy?", "text": "$summary", "buttons": ["Deploy", "Cancel"] } },
    { "#apply": { "on": [{ "$answer": "Deploy" }] } }
] }
```

- **The message.** Each click on a notification from `#notify` sends
  `itero.notification.clicked` to the workspace that showed it, with:

  | Field | Is |
  |---|---|
  | `notification` | the notification's id, new for each one |
  | `answer` | the button's label; `open` for a click on the notification itself; `dismissed` when it is closed without one (only for one that waits) |
  | `workflow` | the workflow whose step showed it |
  | `data` | `#notify`'s `$data`, when it has one: an object it passes on |

  It is one of itero's own (`itero.*`): `itero emit`, `POST /events` and
  sources cannot send it, so no message from outside fakes an answer. It is
  saved in the journal even when the daemon is not running, and handled on
  its next pass.
- **Starting on it.** A workflow starts on it like on any message:
  `{"event": "itero.notification.clicked"}`, with conditions to pick its
  own, `{"$workflow": "gmail"}`. `$data` brings what the click is about
  (a mail's id, say), as the clicked notification gave it.
- **Waiting for it.** `#notify` with `buttons` (a list of labels), or with
  `"wait": true`, waits: it shows the notification, puts `notification`
  (its id) in the run's state and says "not yet", as an action exiting 75
  does. The click's message matches the run by that id, and its `answer`
  joins the state; the step ends and the run goes on. Run again while
  waiting (every hour), it shows nothing again. As for any message, its
  other fields must agree with the state where they share a name, so a
  state with its own `workflow` or `data` is not reached: the reason the
  message carries so few.
  A workflow's `timeout` ends a run nobody answers, as for any wait.
- **Buttons.** Labels as given, at most 4, shown on the notification; how
  macOS lays them out depends on the style chosen for itero in System
  Settings (Banners or Alerts), to be checked in task 3. `$url`, when set,
  is still opened by a click on the notification itself.
- **Not in this plan:** Linux (`notify-send --action` answers only while
  it runs, so the step would block); text typed into a notification;
  buttons that open different urls.

## How it works

1. `#notify` gives the notification an id (UUID v7) and puts in its
   `userInfo`: that id, the workspace's folder, the `itero` that showed it,
   the workflow, `$url` and `$data`. With buttons it registers a category
   for those labels (one per set of labels), with `customDismissAction`
   when it waits, so a dismissal is told too.
2. A click starts Itero.app with no arguments (as today). Its delegate
   reads the `userInfo`, opens the url (as today), and runs `<itero>
   notification-clicked '<json>'` in the workspace's folder: a hidden
   command that saves the message as itero's own. A notification from an
   older itero, with no workspace in it, is only opened, as today.
3. The waiting step: `#notify` returns `NotYet({"notification": id})` the
   first time; run again, it finds `notification` and no `answer` in the
   state and returns `NotYet({})` without showing anything; with an
   `answer`, `Done({})`.

## Tasks

1. **The hidden command.** `itero notification-clicked '<json>'`: checks
   the data is an object with a `notification`, saves
   `itero.notification.clicked` with `events::own`. Test: it starts a
   workflow on the type; `itero emit itero.notification.clicked` is still
   refused.
2. **`#notify` waits.** `StepRun` gets the workflow's name. `#notify` reads
   `$buttons` (a JSON list of labels; more than 4, or not text, is an
   error), `$wait` and `$data`; makes the id; returns `NotYet` as above.
   `ITERO_NOTIFY_LOG` lines add `buttons=` and `notification=`, so tests
   can see the id and answer with the hidden command. Tests (through
   `daemon --once`): a run waits on a notification with buttons; the
   hidden command's message with its id and `answer: "Deploy"` ends the
   wait and `#apply` runs; another id reaches nothing; a run again while
   waiting logs no second notification.
3. **Itero.app.** `itero-notifier` takes `--id`, `--workspace`, `--itero`,
   `--workflow`, `--data`, `--button` (repeated) and `--wait`; puts them in
   `userInfo`; registers the category. The click delegate maps the
   response (default action: `open`; dismiss action: `dismissed`; a
   button: its label) and runs the hidden command. Checked by hand on
   macOS: a click, each button, a dismissal.
4. **Docs and an example.** `#notify` in the actions guide (buttons, wait,
   `$answer`, `$data`), `itero.notification.clicked` in the messages
   guide and its list of itero's own messages. The gmail example passes
   the mail's UID as `$data`, ready for a workflow that marks it read.

## Open questions

- **Names** (mine, for you to take or change): `itero.notification.clicked`,
  `answer` with `open` and `dismissed`, `buttons`, `wait`, `data`.
- **A click on a notification that does not wait** sends the message too,
  so any workflow can react to any click. The other way: only those with
  `data` or buttons send one.
