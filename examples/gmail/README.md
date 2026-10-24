# gmail

A notification for each new Gmail mail, the moment it arrives: no polling.
`connectors/gmail.received` keeps a connection to Gmail open with IMAP
IDLE, and Gmail tells it when a mail comes; it prints one line per mail,
and `workflows/gmail.json` shows it with `#notify`, with a sound: the
subject, under the sender. On macOS a click on
the notification opens the mail in Gmail on the web, and is a message,
`itero.notification.clicked`, whose `data` names the mail (`uid`,
`mailbox`): a workflow on it could mark the mail read.

It needs Python 3.14 or later, and a Gmail app password: turn on 2-Step
Verification for your Google account, then make one at
<https://myaccount.google.com/apppasswords>. Your own password does not
work over IMAP.

```sh
itero args gmail.received '{"user": "you@gmail.com", "password": "$$GMAIL_PASS"}'
itero secret GMAIL_PASS     # the app password, without its spaces
itero daemon
```

The first start only notes where the inbox is; each mail after that is
told once, across restarts. `host`, `port`, `tls` and `mailbox` in the
args point it at another IMAP server or folder.
