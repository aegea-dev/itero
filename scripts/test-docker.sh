#!/bin/sh
# Runs the tests that need Docker: tests/containers.rs (container actions)
# and tests/mail_servers.rs, against a real mail server: GreenMail (Apache
# 2.0), in Docker, with SMTP on 13025, POP3 on 13110 and IMAP on 13143 of
# 127.0.0.1. Its login checks are off: any account exists once used. The
# server's container is removed afterwards, whatever the tests did.
#
# Docker: the `docker` CLI as configured (set DOCKER_CONTEXT to pick one).
set -eu

IMAGE="greenmail/standalone:2.1.2"
NAME="itero-greenmail-test"
TOOLCHAIN=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)

docker rm -f "$NAME" >/dev/null 2>&1 || true
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT INT TERM

docker run -d --rm --name "$NAME" \
	-e GREENMAIL_OPTS='-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.auth.disabled' \
	-p 127.0.0.1:13025:3025 -p 127.0.0.1:13110:3110 -p 127.0.0.1:13143:3143 \
	"$IMAGE" >/dev/null

# up when IMAP answers its greeting
i=0
until printf 'a LOGOUT\r\n' | nc -w 1 127.0.0.1 13143 2>/dev/null | grep -q '^\* OK'; do
	i=$((i + 1))
	if [ "$i" -gt 60 ]; then
		echo "GreenMail did not start; its log:" >&2
		docker logs "$NAME" >&2
		exit 1
	fi
	sleep 1
done

ITERO_DOCKER=1 ITERO_GREENMAIL=127.0.0.1 cargo "+$TOOLCHAIN" test --test containers --test mail_servers
