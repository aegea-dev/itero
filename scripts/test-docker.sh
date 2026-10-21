#!/bin/sh
# Runs the tests that need Docker:
# tests/mail_servers.rs, against a real mail server: GreenMail (Apache
# 2.0), in Docker, with SMTP on 13025, POP3 on 13110 and IMAP on 13143 of
# 127.0.0.1. Its login checks are off: any account exists once used. The
# server's container is removed afterwards, whatever the tests did.
#
# Containers reach this machine as host.docker.internal, which
# --add-host=host-gateway points at the bridge network's gateway. Docker
# Desktop forwards that to 127.0.0.1; a Linux Docker Engine does not, so
# where the gateway is an address of this machine the ports are published
# there too: reachable from containers, still not from the network.
#
# Docker: the `docker` CLI as configured (set DOCKER_CONTEXT to pick one).
set -eu

IMAGE="greenmail/standalone:2.1.2"
NAME="itero-greenmail-test"
TOOLCHAIN=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)

docker rm -f "$NAME" >/dev/null 2>&1 || true
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT INT TERM

addresses=127.0.0.1
gateway=$(docker network inspect bridge -f '{{range .IPAM.Config}}{{.Gateway}}{{end}}')
if [ -n "$gateway" ] && command -v ip >/dev/null 2>&1 &&
	ip -o -4 addr show | grep -q " $gateway/"; then
	addresses="$addresses $gateway"
fi
ports=""
for a in $addresses; do
	ports="$ports -p $a:13025:3025 -p $a:13110:3110 -p $a:13143:3143"
done

# $ports unquoted: one word per flag
# shellcheck disable=SC2086
docker run -d --rm --name "$NAME" \
	-e GREENMAIL_OPTS='-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.auth.disabled' \
	$ports \
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

ITERO_DOCKER=1 ITERO_GREENMAIL=127.0.0.1 cargo "+$TOOLCHAIN" test --test mail_servers
