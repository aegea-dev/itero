#!/usr/bin/env python3
"""imap.check@1: what is new in an IMAP mailbox since the last check.

The job comes on stdin as one line of JSON. Its "with" gives the server
(host, port, tls), the account (user, password), the mailbox (INBOX unless
given) and seen: what the last check printed as "seen", or nothing.

Prints {"new": n, "seen": {"uidvalidity": v, "uid": u}, "summary": "..."}.
The first check (nothing seen) only notes where the mailbox is, so mail
that arrives after it is new; a mailbox whose UIDVALIDITY changed (rebuilt
by the server) starts again the same way. Only the standard library: the
password comes with the job on stdin, never on a command line.
"""
import email
import email.header
import imaplib
import json
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


def quoted(name):
    return '"' + name.replace("\\", "\\\\").replace('"', '\\"') + '"'


def uids(conn, criteria):
    typ, data = conn.uid("SEARCH", None, criteria)
    if typ != "OK":
        raise RuntimeError(f"SEARCH {criteria}: {data}")
    return [int(u) for u in data[0].split()] if data and data[0] else []


def subject(conn, uid):
    typ, data = conn.uid("FETCH", str(uid), "(BODY.PEEK[HEADER.FIELDS (SUBJECT)])")
    if typ != "OK" or not data or not isinstance(data[0], tuple):
        return ""
    msg = email.message_from_bytes(data[0][1])
    return header_text(msg.get("Subject", ""))


def login(conn, user, password):
    """imaplib's LOGIN sends ASCII only. Other letters go as AUTHENTICATE
    PLAIN (UTF-8) when the server offers it, else as LOGIN with the
    password a UTF-8 literal, which most servers take."""
    if (user + password).isascii():
        conn.login(user, password)
    elif "AUTH=PLAIN" in conn.capabilities:
        plain = b"\0" + user.encode() + b"\0" + password.encode()
        conn.authenticate("PLAIN", lambda _: plain)
    else:
        conn.literal = password.encode()
        typ, data = conn._simple_command("LOGIN", conn._quote(user))
        if typ != "OK":
            raise imaplib.IMAP4.error(f"LOGIN: {data}")
        conn.state = "AUTH"


def check(w):
    tls = w.get("tls", True)
    port = int(w.get("port") or (993 if tls else 143))
    conn = (imaplib.IMAP4_SSL if tls else imaplib.IMAP4)(w["host"], port, timeout=30)
    try:
        login(conn, w["user"], w["password"])
        typ, data = conn.select(quoted(w.get("mailbox") or "INBOX"), readonly=True)
        if typ != "OK":
            raise RuntimeError(f"cannot open {w.get('mailbox') or 'INBOX'}: {data}")
        validity = int(conn.response("UIDVALIDITY")[1][0])
        seen = w.get("seen") or {}
        if seen.get("uidvalidity") != validity:
            # nothing seen yet, or a rebuilt mailbox: start from here
            last = max(uids(conn, "ALL"), default=0)
            return {"new": 0, "seen": {"uidvalidity": validity, "uid": last}, "summary": ""}
        last = int(seen.get("uid", 0))
        # "n:*" always matches the newest message, even below n
        new = [u for u in uids(conn, f"UID {last + 1}:*") if u > last]
        if not new:
            return {"new": 0, "seen": seen, "summary": ""}
        newest = max(new)
        return {
            "new": len(new),
            "seen": {"uidvalidity": validity, "uid": newest},
            "summary": f"{len(new)} new: {subject(conn, newest)}",
        }
    finally:
        try:
            conn.logout()
        except Exception:
            pass


def main():
    job = json.loads(sys.stdin.readline() or "{}")
    try:
        print(json.dumps(check(job.get("with", {})), ensure_ascii=False))
    except (OSError, imaplib.IMAP4.error, RuntimeError, KeyError, ValueError) as e:
        print(f"imap.check@1: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
