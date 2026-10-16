#!/usr/bin/env python3
"""pop3.check@1: what is new in a POP3 mailbox since the last check.

The job comes on stdin as one line of JSON. Its "with" gives the server
(host, port, tls), the account (user, password) and seen: what the last
check printed as "seen" (the messages' unique ids then), or nothing.

Prints {"new": n, "seen": [unique ids], "summary": "..."}. POP3 has no
"since": a message is new when its unique id (UIDL) was not there last
time. The first check (nothing seen) only notes what is there, so mail
that arrives after it is new. "seen" is the mailbox's ids now, so it never
grows past the mailbox. Only the standard library: the password comes with
the job on stdin, never on a command line.
"""
import email
import email.header
import json
import poplib
import sys


def header_text(raw):
    """A header's text, its encoded words decoded."""
    out = []
    for part, charset in email.header.decode_header(raw):
        if isinstance(part, bytes):
            out.append(part.decode(charset or "utf-8", "replace"))
        else:
            out.append(part)
    return "".join(out).strip()


def subject(conn, number):
    _, lines, _ = conn.top(number, 0)
    msg = email.message_from_bytes(b"\r\n".join(lines))
    return header_text(msg.get("Subject", ""))


def check(w):
    tls = w.get("tls", True)
    port = int(w.get("port") or (995 if tls else 110))
    conn = (poplib.POP3_SSL if tls else poplib.POP3)(w["host"], port, timeout=30)
    try:
        conn.user(w["user"])
        conn.pass_(w["password"])
        _, lines, _ = conn.uidl()
        now = []
        for line in lines:
            number, uid = line.decode("utf-8", "replace").split(" ", 1)
            now.append((int(number), uid))
        ids = [uid for _, uid in now]
        seen = w.get("seen")
        if not isinstance(seen, list):
            # nothing seen yet: start from here
            return {"new": 0, "seen": ids, "summary": ""}
        known = set(seen)
        new = [(n, uid) for n, uid in now if uid not in known]
        if not new:
            return {"new": 0, "seen": ids, "summary": ""}
        newest = max(n for n, _ in new)
        return {"new": len(new), "seen": ids, "summary": f"{len(new)} new: {subject(conn, newest)}"}
    finally:
        try:
            conn.quit()
        except Exception:
            pass


def main():
    job = json.loads(sys.stdin.readline() or "{}")
    try:
        print(json.dumps(check(job.get("with", {})), ensure_ascii=False))
    except (OSError, poplib.error_proto, KeyError, ValueError) as e:
        print(f"pop3.check@1: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
