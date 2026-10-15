#!/bin/sh
# Prints {"new": n, "seen": "<id of the last message>", "summary": "..."}.
# A message's id is its "From " line (sender and date). The job comes on
# stdin as JSON; its "with" (the last key, as itero writes it) gives the
# mailbox (default: /var/mail/<user>) and the last id seen. A mailbox that
# no longer has the id seen counts every message as new.
job=$(cat)
with=$(printf '%s' "$job" | sed -n 's/.*"with":{\([^}]*\)}.*/\1/p')
field() { printf '%s' "$with" | sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }
mailbox=$(field mailbox)
[ -n "$mailbox" ] || mailbox="/var/mail/$(id -un)"
seen=$(field seen)
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
ids=""
[ -s "$mailbox" ] && ids=$(grep '^From ' "$mailbox")
if [ -z "$ids" ]; then
  printf '{"new": 0, "seen": "%s", "summary": ""}\n' "$(esc "$seen")"
  exit 0
fi
last=$(printf '%s\n' "$ids" | tail -n 1)
new=$(printf '%s\n' "$ids" | awk -v s="$seen" 'f { n++ } $0 == s { f = 1 } END { print f ? n + 0 : NR }')
subject=$(grep '^Subject: ' "$mailbox" | tail -n 1 | sed 's/^Subject: //')
printf '{"new": %s, "seen": "%s", "summary": "%s new: %s"}\n' "$new" "$(esc "$last")" "$new" "$(esc "$subject")"
