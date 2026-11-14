#!/bin/sh
# Measures how fast itero takes events and runs workflows, in a workspace
# of its own in a temporary folder, with `itero ws daemon` (the machine's
# list and daemon are not touched):
#
# - pick-up: an event at a time into an idle daemon, by each way in (a
#   connector's line, the webhook, `itero ws emit`), from the event saved
#   to its run started (ms, from the ids), from it sent to its run started
#   (to the microsecond: the time jq put in it, against the log's), and
#   from start to end, for a 1-step workflow. Sent counts the sender's own
#   start: curl's, `itero ws emit`'s;
# - throughput: BENCH_N events saved while no daemon runs, then a daemon
#   started: runs a second, first start to last end, for 1 and 3 steps;
# - the webhook listener: BENCH_REQUESTS requests, 32 at once, refused
#   (an unlisted path) and saved (a listed path no workflow takes).
#
# Times come from the log (its ids, UUIDv7, in milliseconds; its `time`,
# in microseconds), so they are the engine's own, whatever the polling
# here costs. Each step is a `sh`
# action that reads its input and prints `{"ok":true}`.
#
# ITERO: the itero to measure (default: target/release/itero).
# BENCH_N (default 500), BENCH_K (events a way in, default 20),
# BENCH_REQUESTS (default 2000); BENCH_KEEP=1 keeps the workspace. The
# results, as JSON lines, are added to target/bench/results.jsonl, with
# the version, the system and the time.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
itero=${ITERO:-$root/target/release/itero}
n=${BENCH_N:-500}
k=${BENCH_K:-20}
requests=${BENCH_REQUESTS:-2000}
out=${BENCH_OUT:-$root/target/bench}
token='çalışkan-arı-0123456789'

[ -x "$itero" ] || {
	echo "bench.sh: no itero at $itero (make build, or ITERO=...)" >&2
	exit 1
}
for tool in curl jq; do
	command -v "$tool" >/dev/null || {
		echo "bench.sh: needs $tool" >&2
		exit 1
	}
done
mkdir -p "$out"
ws=$(mktemp -d "${TMPDIR:-/tmp}/itero-bench.XXXXXX")
daemon=
cleanup() {
	if [ -n "$daemon" ]; then
		kill "$daemon" 2>/dev/null || true
		wait "$daemon" 2>/dev/null || true
	fi
	if [ -n "${BENCH_KEEP:-}" ]; then echo "bench.sh: kept $ws" >&2; else rm -rf "$ws"; fi
}
trap cleanup EXIT INT TERM

# --- the workspace ---------------------------------------------------------
mkdir -p "$ws/actions" "$ws/workflows" "$ws/connectors/bench" "$ws/connectors/webhook"
echo '{ "name": "bench", "description": "How fast events are taken and runs end" }' >"$ws/itero.config.json"
echo "BENCH_TOKEN=$token" >"$ws/.env"
for a in a b c; do
	printf '#!/bin/sh\ncat >/dev/null\necho '"'"'{"ok":true}'"'"'\n' >"$ws/actions/$a"
done
fifo="$ws/lines"
mkfifo "$fifo"
# read and write: the fifo never ends, so neither does the connector
printf '#!/bin/sh\nexec cat <>"%s"\n' "$fifo" >"$ws/connectors/bench/bench.sent"
cat >"$ws/connectors/webhook/webhook.listen" <<'EOF'
#!/bin/sh
exec itero webhook --port 0 --hook /hook:token:BENCH_TOKEN --hook /saved:token:BENCH_TOKEN
EOF
chmod +x "$ws"/actions/* "$ws"/connectors/*/*
cat >"$ws/workflows/one.json" <<'EOF'
{ "on": [{ "event": "bench.sent" }, { "$event.wf": "one" }], "steps": ["#a"] }
EOF
cat >"$ws/workflows/three.json" <<'EOF'
{ "on": [{ "event": "bench.sent" }, { "$event.wf": "three" }], "steps": ["#a", "#b", "#c"] }
EOF
cat >"$ws/workflows/hook.json" <<'EOF'
{ "on": [{ "event": "itero.webhook.received" }, { "$event.path": "/hook" }], "steps": ["#a"] }
EOF
log="$ws/.itero/itero.log"

# lines of the log that match, 0 before there is a log
count() {
	c=$(grep -cE "$1" "$log" 2>/dev/null) || :
	echo "${c:-0}"
}
started() { count '"type":"itero.daemon.started"'; }
start_daemon() {
	starts=$(started)
	(cd "$ws" && exec "$itero" ws daemon) >>"$ws/daemon.out" 2>&1 &
	daemon=$!
	i=0
	until [ "$(started)" -gt "$starts" ]; do
		i=$((i + 1))
		[ "$i" -lt 500 ] || {
			echo "bench.sh: the daemon did not start" >&2
			cat "$ws/daemon.out" >&2
			exit 1
		}
		sleep 0.02
	done
}
stop_daemon() {
	kill "$daemon"
	wait "$daemon" 2>/dev/null || true
	daemon=
}
# the bench's runs ended so far (the daemon's own workflows run too)
ended() { count '"event":"run.ended".*"workflow":"(one|three|hook)"'; }
# waits until `count` runs have ended in all, 10 minutes at most
wait_ended() {
	i=0
	while [ "$(ended)" -lt "$1" ]; do
		i=$((i + 1))
		[ "$i" -lt 30000 ] || {
			echo "bench.sh: runs did not end" >&2
			exit 1
		}
		sleep 0.02
	done
}

# Every run of the bench's workflows from the log: its event's tag, when
# the event was saved, when the run started and ended (ms, from the ids).
runs() {
	jq -sc '
		def ms: .[0:8] + .[9:13] | ascii_downcase | explode
			| map(if . >= 97 then . - 87 else . - 48 end)
			| reduce .[] as $d (0; . * 16 + $d);
		# a log time, in seconds; a fraction when it has one
		def secs: capture("^(?<s>[^.Z]*)(?<f>\\.[0-9]+)?Z$")
			| (.s + "Z" | fromdateiso8601) + ((.f // ".0") | "0" + . | tonumber);
		(map(select(.event == "message" and (.type == "bench.sent" or .type == "itero.webhook.received")))
			| map({key: .message, value: {tag: (.data.tag // .data.body.tag), sent: (.data.sent // .data.body.sent),
				saved: (.message | ms)}}) | from_entries) as $events
		| (map(select(.type == "itero.run.done")) | map({key: .data.run, value: (.message | ms)}) | from_entries) as $ends
		# a run a schedule started has no message
		| map(select(.event == "run.started" and .message != null and $events[.message] != null))
		| map($events[.message] + {workflow, start: (.run | ms), started: (.time | secs), end: $ends[.run]})
		| map(select(.end != null))' "$log"
}
# p50 and p95 of a list of numbers
stats='sort | {p50: .[(length * 0.5 | floor)], p95: .[([(length * 0.95 | floor), length - 1] | min)]}'

# --- pick-up, an event at a time -------------------------------------------
start_daemon
i=0
until port=$(grep -o 'listening on 127\.0\.0\.1:[0-9]*' "$ws/daemon.out" 2>/dev/null | head -1 | sed 's/.*://') && [ -n "$port" ]; do
	i=$((i + 1))
	[ "$i" -lt 500 ] || {
		echo "bench.sh: the webhook did not start; the log ends:" >&2
		tail -5 "$ws/daemon.out" >&2
		exit 1
	}
	sleep 0.02
done
hook="http://127.0.0.1:$port"
# `itero ws emit`: its warnings about the machine's daemon (this one runs
# alone) go to a file, shown when it fails
emit() {
	(cd "$ws" && "$itero" ws emit bench.sent "$1" >/dev/null 2>>"$ws/emit.err") || {
		cat "$ws/emit.err" >&2
		exit 1
	}
}
send() {
	case $1 in
	# jq puts the time it is sent in it: `now`, in seconds, to the microsecond
	connector) jq -nc '{wf: "one", tag: "connector", sent: now}' >"$fifo" ;;
	webhook) curl -fsS -o /dev/null -X POST -H "Authorization: Bearer $token" -H 'Content-Type: application/json' \
		-d "$(jq -nc '{tag: "webhook", sent: now}')" "$hook/hook" ;;
	emit) emit "$(jq -nc '{wf: "one", tag: "emit", sent: now}')" ;;
	esac
}
sleep 1
for way in connector webhook emit; do
	j=0
	while [ "$j" -lt "$k" ]; do
		before=$(ended)
		send "$way"
		wait_ended $((before + 1))
		sleep 0.05
		j=$((j + 1))
	done
done

# --- the webhook listener ----------------------------------------------------
listener() { # path, label
	cfg="$ws/curl-$2"
	: >"$cfg"
	j=0
	while [ "$j" -lt "$requests" ]; do
		printf 'url = "%s%s"\n' "$hook" "$1" >>"$cfg"
		j=$((j + 1))
	done
	t0=$(jq -n now)
	curl -s --parallel --parallel-max 32 --config "$cfg" -X POST \
		-H "Authorization: Bearer $token" -H 'Content-Type: application/json' -d '{"tag":"listener"}' \
		-o /dev/null -w '%{http_code} %{time_total}\n' >"$cfg.out" 2>/dev/null || true
	t1=$(jq -n now)
	jq -Rsc --arg label "$2" --argjson secs "$(jq -n "$t1 - $t0")" '
		split("\n") | map(select(length > 0) | split(" ")) as $r
		| {test: "listener", what: $label, requests: ($r | length),
			codes: ($r | group_by(.[0]) | map({key: .[0][0], value: length}) | from_entries),
			seconds: ($secs * 1000 | round / 1000),
			ms: ($r | map(.[1] | tonumber * 1000 | round) | '"$stats"')}' "$cfg.out"
}
listener /none refused >"$ws/listener.jsonl"
listener /saved saved >>"$ws/listener.jsonl"
stop_daemon

# --- throughput ---------------------------------------------------------------
for wf in one three; do
	seq 1 "$n" | sed "s/.*/{\"wf\":\"$wf\",\"tag\":\"load-$wf\"}/" |
		tr "\n" "\0" | (cd "$ws" && xargs -0 -P 4 -I{} "$itero" ws emit bench.sent {} >/dev/null 2>>"$ws/emit.err") || {
		cat "$ws/emit.err" >&2
		exit 1
	}
	before=$(ended)
	start_daemon
	wait_ended $((before + n))
	stop_daemon
done

# --- results ------------------------------------------------------------------
version=$("$itero" --version | sed 's/^itero //')
system="$(uname -s) $(uname -m), $(getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null || sysctl -n hw.ncpu) CPUs"
at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
runs >"$ws/runs.json"
{
	for way in connector webhook emit; do
		jq -c --arg way "$way" '
			map(select(.tag == $way))
			| {test: "pick-up", what: $way, runs: length,
				saved_to_start_ms: (map(.start - .saved) | '"$stats"'),
				sent_to_start_ms: (map(select(.sent != null) | (.started - .sent) * 1000000 | round / 1000) | '"$stats"'),
				start_to_end_ms: (map(.end - .start) | '"$stats"')}' "$ws/runs.json"
	done
	for wf in one three; do
		jq -c --arg wf "$wf" '
			map(select(.tag == "load-" + $wf))
			| ((map(.end) | max) - (map(.start) | min)) as $span
			| {test: "throughput", what: $wf, runs: length, span_ms: $span,
				runs_per_second: (length * 1000 / $span | round)}' "$ws/runs.json"
	done
	cat "$ws/listener.jsonl"
} | jq -c --arg version "$version" --arg system "$system" --arg at "$at" \
	'{at: $at, version: $version, system: $system} + .' | tee -a "$out/results.jsonl" |
	jq -r '
		if .test == "pick-up" then
			"pick-up     \(.what | . + "         " | .[0:10]) saved->start p50 \(.saved_to_start_ms.p50) ms p95 \(.saved_to_start_ms.p95) ms, sent->start p50 \(.sent_to_start_ms.p50) ms, start->end p50 \(.start_to_end_ms.p50) ms (\(.runs) runs)"
		elif .test == "throughput" then
			"throughput  \(.what | . + "         " | .[0:10]) \(.runs_per_second) runs/s (\(.runs) runs in \(.span_ms) ms)"
		else
			"listener    \(.what | . + "         " | .[0:10]) \(.requests / ([.seconds, 0.001] | max) | round) req/s, p50 \(.ms.p50) ms p95 \(.ms.p95) ms, codes \(.codes | tostring)"
		end'
echo "added to $out/results.jsonl"
