# messages

An alert by mail, by push notification or by iMessage: one action for
each, all taking the same args (`to`, `subject`, `text`), so a workflow
changes the way by changing a step's name. Here the event says which:

```sh
itero ws emit alert.send '{"via": "push", "subject": "Yedek", "text": "Gece yedeği bitti."}'
itero ws emit alert.send '{"via": "mail", "subject": "Yedek", "text": "Gece yedeği bitti."}'
itero ws emit alert.send '{"via": "imessage", "subject": "Yedek", "text": "Gece yedeği bitti."}'
```

Each costs nothing beyond what you have, and needs only curl (and jq,
for push):

- **`#send-mail`** sends through your own mail server over SMTP: set the
  host and addresses in `workflows/alert.json`, and keep the password as a
  secret, `itero secret SMTP_PASS` (for Gmail or iCloud, an app password).
  Port 465 by default; another `port` uses STARTTLS.
- **`#push`** sends to an [ntfy](https://ntfy.sh) topic; the ntfy app on
  your phone, subscribed to it, shows it. Anyone who knows a topic on
  ntfy.sh can read it, so make it a long random name and keep it as a
  secret: `itero secret NTFY_TOPIC`. `server` points at your own ntfy
  server, `token` gives its access token.
- **`#imessage`** sends through Messages on this Mac, from your Apple
  Account: set `to` to a phone number or an address. With `service` set
  to `SMS`, it sends an SMS from your iPhone's number instead (Text
  Message Forwarding on, in the iPhone's settings). macOS asks once
  whether the program running it may control Messages.

Run `itero ws daemon` in this folder. The tests send mail to a mail server in
Docker (`make test-docker`) and push to a stand-in for ntfy; iMessage is
not tested, as it would send from your account.
