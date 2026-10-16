#!/usr/bin/env python3
"""imap-idle: tells itero when mail arrives in an IMAP mailbox, as it
arrives (IMAP IDLE, RFC 2177), so a check runs at once instead of on the
next schedule.

An itero source: it reads its settings as one line of JSON on stdin (its
"with" gives host, port, tls, user, password and mailbox), and prints one
line of JSON per change, {"data": {"mailbox": ..., "exists": n}}, whose
type is the source's "emit". It prints one at start too, so a check runs
and notes where the mailbox is. It renews IDLE every 25 minutes (servers
end it after 30), and on a server without IDLE asks NOOP every 60 seconds
instead. It exits when stdin closes: the daemon has gone.

A small client on a socket, standard library only: imaplib has IDLE only
from Python 3.14. It logs in with literals, which carry any password as
UTF-8.
"""
import json
import os
import socket
import ssl
import sys
import threading
import time

RENEW = 25 * 60
POLL = 60


class Closed(Exception):
    pass


class Imap:
    def __init__(self, host, port, tls):
        sock = socket.create_connection((host, port), timeout=30)
        if tls:
            sock = ssl.create_default_context().wrap_socket(sock, server_hostname=host)
        self.sock = sock
        self.buf = b""
        self.n = 0

    def line(self, timeout=30):
        """The next line, without its CRLF; None when `timeout` ran out."""
        while b"\r\n" not in self.buf:
            self.sock.settimeout(timeout)
            try:
                data = self.sock.recv(65536)
            except socket.timeout:
                return None
            if not data:
                raise Closed("the server closed the connection")
            self.buf += data
        line, self.buf = self.buf.split(b"\r\n", 1)
        return line.decode("utf-8", "replace")

    def send(self, data):
        self.sock.sendall(data)

    def tag(self):
        self.n += 1
        return f"a{self.n}"

    def until_done(self, tag, seen=None):
        """Reads up to the tagged answer; each untagged line goes to `seen`."""
        while True:
            line = self.line()
            if line is None:
                raise Closed(f"no answer to {tag}")
            if line.startswith(tag + " "):
                if not line[len(tag) + 1:].startswith("OK"):
                    raise RuntimeError(line)
                return
            if seen:
                seen(line)

    def literal(self, data):
        """Sends a literal, {n} then the bytes once the server says go on."""
        self.send(b"{%d}\r\n" % len(data))
        while True:
            line = self.line()
            if line is None or not line.startswith("+"):
                if line is not None and line.startswith("*"):
                    continue
                raise RuntimeError(f"the server took no literal: {line}")
            break
        self.send(data)


def exists(line):
    parts = line.split()
    if len(parts) == 3 and parts[0] == "*" and parts[2].upper() == "EXISTS":
        return int(parts[1])
    return None


def emit(mailbox, n):
    print(json.dumps({"data": {"mailbox": mailbox, "exists": n}}), flush=True)


def quoted(name):
    return '"' + name.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main():
    settings = json.loads(sys.stdin.readline() or "{}")

    def stop_when_stdin_closes():
        sys.stdin.read()
        os._exit(0)

    threading.Thread(target=stop_when_stdin_closes, daemon=True).start()

    w = settings.get("with", {})
    tls = w.get("tls", True)
    port = int(w.get("port") or (993 if tls else 143))
    mailbox = w.get("mailbox") or "INBOX"
    imap = Imap(w["host"], port, tls)
    greeting = imap.line()
    if greeting is None or not greeting.startswith("* OK"):
        raise RuntimeError(f"no greeting: {greeting}")

    caps = set()

    def capability(line):
        if line.upper().startswith("* CAPABILITY "):
            caps.update(line.upper().split()[2:])

    tag = imap.tag()
    imap.send(f"{tag} LOGIN ".encode())
    imap.literal(w["user"].encode())
    imap.send(b" ")
    imap.literal(w["password"].encode())
    imap.send(b"\r\n")
    imap.until_done(tag, capability)
    tag = imap.tag()
    imap.send(f"{tag} CAPABILITY\r\n".encode())
    imap.until_done(tag, capability)

    count = [0]

    def selected(line):
        n = exists(line)
        if n is not None:
            count[0] = n

    tag = imap.tag()
    imap.send(f"{tag} EXAMINE {quoted(mailbox)}\r\n".encode())
    imap.until_done(tag, selected)
    # a check now notes where the mailbox is
    emit(mailbox, count[0])

    def changed(line):
        n = exists(line)
        if n is not None:
            if n > count[0]:
                emit(mailbox, n)
            count[0] = n

    if "IDLE" not in caps:
        while True:
            time.sleep(POLL)
            tag = imap.tag()
            imap.send(f"{tag} NOOP\r\n".encode())
            imap.until_done(tag, changed)

    while True:
        tag = imap.tag()
        imap.send(f"{tag} IDLE\r\n".encode())
        line = imap.line()
        while line is not None and not line.startswith("+"):
            changed(line)
            line = imap.line()
        if line is None:
            raise Closed("IDLE was not taken")
        renew_at = time.monotonic() + RENEW
        while True:
            line = imap.line(timeout=max(1, renew_at - time.monotonic()))
            if line is None:
                break
            changed(line)
        imap.send(b"DONE\r\n")
        imap.until_done(tag, changed)


if __name__ == "__main__":
    try:
        main()
    except (OSError, Closed, RuntimeError, KeyError, ValueError) as e:
        print(f"imap-idle: {e}", file=sys.stderr, flush=True)
        os._exit(1)
