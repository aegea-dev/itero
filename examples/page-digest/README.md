# page-digest

A page, summarized by an AI assistant, by mail, every morning at 9.
Set the page, the mail server and the addresses in
workflows/page-digest.json; keep the keys as secrets:
  itero secret ANTHROPIC_KEY
  itero secret SMTP_PASS
The mail goes over SMTP on port 465; another `port` uses STARTTLS.
The actions need curl and jq. Run `itero ws daemon` in this folder.
