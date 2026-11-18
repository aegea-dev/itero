#!/bin/sh
# Runs one workspace that uses every part of the engine under a steady
# load for a long time, and records how it holds: whether it keeps up,
# and what grows. In a workspace of its own in a temporary folder, with
# `itero ws daemon` (the machine's list and daemon are not touched).
#
# The workspace: the webhook -> `api` (1 step); a connector's lines ->
# `order` (3 steps, one of them the workflow `ship` used as a step);
# `slow` (one step that waits 200 ms); `warm` (one Python step, run
# warm: its action kept loaded between steps); `tick`, a schedule every
# minute; and the built-in `keep` (#forget) and `logs` (#roll-log), with
# the default limits but for those SOAK_LIMITS sets (default: the log rolled
# at 100MB, not 10MB, so it is not rolled twice between two looks at the
# burst's rate).
#
# SOAK_MODE=full (default), in phases:
# - sustained: SOAK_RATE events a second (default 116: 10 million a day),
#   50% api, 30% order, 10% slow, 10% warm, for SOAK_SECS (default
#   3600); halfway, SOAK_BURST a second (default 1200) for
#   SOAK_BURST_SECS (default 60);
# - drain: nothing sent; until nothing waits (SOAK_DRAIN_MAX s at most,
#   default 1800);
# - restart: the daemon stopped, SOAK_BACKLOG events (default 20000)
#   queued with `itero ws emit`, the daemon started: the time to its
#   first run and until nothing waits (SOAK_DRAIN_MAX s at most).
# SOAK_MODE=slow: only `slow`, 2000 queued (SOAK_SLOW_N), at
# `limits.steps` 8, 64 and 512 (SOAK_SLOW_STEPS): runs a second against
# the ceiling of steps / 0.2 s.
#
# Every SOAK_EVERY seconds (default 10) a line in samples.jsonl: what was
# sent and refused, runs started and ended, what waits, saved -> started
# and started -> ended (p50, p95, ms), the daemon's memory and open files,
# the journal, its WAL, the logs, the files in .itero/jobs, connector
# restarts, and each warm copy's Python process: its memory (KB), and
# its worker's (`itero worker --warm`, the daemon's child it runs
# under). The log is read from where it was left, a rolled one too, so
# a long run costs the same each time. Results in
# SOAK_OUT/<UTC time>/ (default target/soak): samples.jsonl,
# results.jsonl, summary.json, and the daemon's own output.
#
# ITERO: the itero to run (default target/release/itero). Every phase has
# a time limit: the run ends, and says why, rather than waiting forever.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
itero=${ITERO:-$root/target/release/itero}
mode=${SOAK_MODE:-full}
secs=${SOAK_SECS:-3600}
rate=${SOAK_RATE:-116}
burst=${SOAK_BURST:-1200}
burst_secs=${SOAK_BURST_SECS:-60}
backlog=${SOAK_BACKLOG:-20000}
drain_max=${SOAK_DRAIN_MAX:-1800}
every=${SOAK_EVERY:-10}
limits=${SOAK_LIMITS:-'{"log": "100MB"}'}
slow_n=${SOAK_SLOW_N:-2000}
slow_steps=${SOAK_SLOW_STEPS:-8 64 512}
out=${SOAK_OUT:-$root/target/soak}/$(date -u +%Y%m%dT%H%M%SZ)
token='çalışkan-arı-0123456789'

[ -x "$itero" ] || {
	echo "soak.sh: no itero at $itero (make build, or ITERO=...)" >&2
	exit 1
}
for tool in curl jq python3; do
	command -v "$tool" >/dev/null || {
		echo "soak.sh: needs $tool" >&2
		exit 1
	}
done
case $mode in full | slow) ;; *)
	echo "soak.sh: SOAK_MODE is full or slow, not $mode" >&2
	exit 1
	;;
esac
mkdir -p "$out"
say() { echo "soak.sh: $(date -u +%H:%M:%S) $*" | tee -a "$out/soak.out" >&2; }
# a line of results.jsonl: what a phase found
result() { jq -nc "$@" | tee -a "$out/results.jsonl" >&2; }
now() { date +%s; }

ws=$(mktemp -d "${TMPDIR:-/tmp}/itero-soak.XXXXXX")
# the daemon puts its own folder first on its connectors' PATH, and the
# webhook's connector runs `itero` by that name: a copy, a file of its
# own (on Linux a link would be resolved to the itero it points at)
mkdir "$ws/bin"
cp "$itero" "$ws/bin/itero"
itero="$ws/bin/itero"
daemon='' driver='' sampler='' starts=0
cleanup() {
	for p in $driver $sampler; do kill "$p" 2>/dev/null || true; done
	if [ -n "$daemon" ]; then
		kill "$daemon" 2>/dev/null || true
		wait "$daemon" 2>/dev/null || true
	fi
	# each start's own output, whole: daemon.1.out, daemon.2.out
	for f in "$ws"/daemon.*.out; do [ -f "$f" ] && cp "$f" "$out/"; done
	if [ -n "${SOAK_KEEP:-}" ]; then say "kept $ws"; else rm -rf "$ws"; fi
}
trap cleanup EXIT
trap 'say "stopped"; exit 130' INT TERM

# --- the workspace -----------------------------------------------------------
mkdir -p "$ws/actions" "$ws/workflows" "$ws/connectors/soak" "$ws/connectors/webhook"
jq -n --argjson limits "$limits" \
	'{name: "soak", description: "The whole engine under load, for a long time", limits: $limits}' \
	>"$ws/itero.config.json"
echo "SOAK_TOKEN=$token" >"$ws/.env"
for a in a check pack close; do
	printf '#!/bin/sh\ncat >/dev/null\necho '"'"'{"ok":true}'"'"'\n' >"$ws/actions/$a"
done
printf '#!/bin/sh\ncat >/dev/null\nsleep 0.2\necho '"'"'{"ok":true}'"'"'\n' >"$ws/actions/wait"
# plain Python, no packages, as examples/languages/actions/python/price
cat >"$ws/actions/py" <<'EOF'
#!/usr/bin/env python3
import json
import sys

json.load(sys.stdin)
print(json.dumps({"ok": True}))
EOF
fifo="$ws/lines"
mkfifo "$fifo"
# read and write: the fifo never ends, so neither does the connector
printf '#!/bin/sh\nexec cat <>"%s"\n' "$fifo" >"$ws/connectors/soak/soak.sent"
cat >"$ws/connectors/webhook/webhook.listen" <<'EOF'
#!/bin/sh
exec itero webhook --port 0 --hook /api:token:SOAK_TOKEN
EOF
chmod +x "$ws"/actions/* "$ws"/connectors/*/*
echo '{ "on": [{ "event": "itero.webhook.received" }, { "$event.path": "/api" }], "steps": ["#a"] }' >"$ws/workflows/api.json"
echo '{ "on": [{ "event": "soak.sent" }, { "$event.wf": "order" }], "steps": ["#check", "ship", "#close"] }' >"$ws/workflows/order.json"
echo '{ "steps": ["#pack"] }' >"$ws/workflows/ship.json"
echo '{ "on": [{ "event": "soak.sent" }, { "$event.wf": "slow" }], "steps": ["#wait"] }' >"$ws/workflows/slow.json"
echo '{ "on": [{ "event": "soak.sent" }, { "$event.wf": "warm" }], "steps": [{ "#py": { "warm": true } }] }' >"$ws/workflows/warm.json"
echo '{ "on": [{ "schedule": "* * * * *" }], "steps": ["#a"] }' >"$ws/workflows/tick.json"
log="$ws/.itero/itero.log"
export ITERO_NO_SECRET_STORE=1

# --- the daemon --------------------------------------------------------------
# starts it, and waits (60 s at most) for its API and, unless told not to,
# its webhook
start_daemon() { # [--no-serve]
	starts=$((starts + 1))
	dout="$ws/daemon.$starts.out"
	(cd "$ws" && exec "$itero" ws daemon "$@") >"$dout" 2>&1 &
	daemon=$!
	api='' hook=''
	i=0
	while :; do
		if [ $# -eq 0 ] && [ -z "$api" ]; then
			api=$(grep -o 'serving on http://127\.0\.0\.1:[0-9]*' "$dout" | head -1 | sed 's/.*serving on //') || :
		fi
		if [ -z "$hook" ]; then
			hook=$(grep -o 'listening on 127\.0\.0\.1:[0-9]*' "$dout" | head -1 | sed 's/.*listening on /http:\/\//') || :
		fi
		{ [ $# -gt 0 ] || [ -n "$api" ]; } && [ -n "$hook" ] && break
		i=$((i + 1))
		[ "$i" -lt 600 ] || {
			cat "$dout" >&2
			say "the daemon did not start in 60 s"
			exit 1
		}
		kill -0 "$daemon" 2>/dev/null || {
			cat "$dout" >&2
			say "the daemon ended as it started"
			exit 1
		}
		sleep 0.1
	done
}
stop_daemon() {
	kill "$daemon"
	wait "$daemon" 2>/dev/null || true
	daemon=
}

# --- the sampler -------------------------------------------------------------
bytes() { if [ -f "$1" ]; then wc -c <"$1" | tr -d ' '; else echo 0; fi; }
# UUIDv7 ids carry their time in ms: an event's when it was saved, a run's
# when it started
jq_defs='
	def ms: .[0:8] + .[9:13] | ascii_downcase | explode
		| map(if . >= 97 then . - 87 else . - 48 end)
		| reduce .[] as $d (0; . * 16 + $d);
	def secs: capture("^(?<s>[^.Z]*)(?<f>\\.[0-9]+)?Z$")
		| (.s + "Z" | fromdateiso8601) + ((.f // ".0") | "0" + . | tonumber);
	def pct: sort | if length == 0 then null else
		{p50: .[(length * 0.5 | floor)], p95: .[([(length * 0.95 | floor), length - 1] | min)]} end;'
# the log's whole lines written since the last look: where it was left
# and which file it was are kept in files, since the sampler runs apart;
# a line still being written is left for the next look; a rolled log
# (another file under the name) is read to its end first
new_lines() {
	offset=$(cat "$ws/offset")
	inode=$(cat "$ws/inode")
	now_inode=$(ls -i "$log" 2>/dev/null | awk '{print $1}')
	[ -n "$now_inode" ] || return 0
	if [ -n "$inode" ] && [ "$now_inode" != "$inode" ]; then
		tail -c +"$((offset + 1))" "$log.1" 2>/dev/null || :
		offset=0
	fi
	size=$(bytes "$log")
	if [ "$size" -gt "$offset" ]; then
		head -c "$size" "$log" | tail -c +"$((offset + 1))" >"$ws/chunk"
		got=$(bytes "$ws/chunk")
		if [ "$(tail -c 1 "$ws/chunk" | wc -l | tr -d ' ')" -eq 0 ]; then
			got=$((got - $(tail -n 1 "$ws/chunk" | wc -c | tr -d ' ')))
		fi
		head -c "$got" "$ws/chunk"
		offset=$((offset + got))
	fi
	echo "$offset" >"$ws/offset"
	echo "$now_inode" >"$ws/inode"
}
mem_files() { # the daemon's memory (KB) and open files
	if [ -r "/proc/$1/status" ]; then
		rss=$(awk '/^VmRSS/ {print $2}' "/proc/$1/status")
		fds=$(ls "/proc/$1/fd" 2>/dev/null | wc -l | tr -d ' ')
	else
		rss=$(ps -o rss= -p "$1" | tr -d ' ')
		fds=$(lsof -p "$1" 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
	fi
	echo "${rss:-0} ${fds:-0}"
}
# every process as "pid ppid rss(KB) command", from /proc on Linux (as
# mem_files reads it) and ps elsewhere
procs() {
	if [ -d /proc/self ]; then
		for d in /proc/[0-9]*; do
			awk -v pid="${d#/proc/}" -v cmd="$(tr '\0' ' ' <"$d/cmdline" 2>/dev/null)" '
				/^PPid:/ { ppid = $2 } /^VmRSS:/ { rss = $2 }
				END { if (ppid != "") print pid, ppid, rss + 0, cmd }' "$d/status" 2>/dev/null || :
		done
	else
		ps -A -o pid=,ppid=,rss=,command=
	fi
}
# the warm copies, as a JSON array: each one's Python process (the child
# of an `itero worker --warm` the daemon started), its pid and memory
# (KB), and its worker's memory
warm_copies() {
	procs | awk -v d="$1" '
		{ ppid[$1] = $2; rss[$1] = $3; cmd = $0; sub(/^ *[0-9]+ +[0-9]+ +[0-9]+ */, "", cmd); line[$1] = cmd }
		END {
			n = 0
			for (p in ppid) {
				w = ppid[p]
				if (ppid[w] == d && line[w] ~ /worker --warm/)
					printf "%s{\"pid\":%s,\"rss_kb\":%s,\"worker_rss_kb\":%s}", (n++ ? "," : "["), p, rss[p], rss[w]
			}
			print (n ? "]" : "[]")
		}'
}
sample() {
	t=$(now)
	lines=$(new_lines | jq -c "$jq_defs"'
		select(.event == "run.started" or .event == "run.ended"
			or (.event == "message" and (.type == "itero.space.full" or .type == "itero.log.full")))' 2>/dev/null || :)
	mf=$(mem_files "$daemon")
	rss=${mf% *} fds=${mf#* }
	logs=0
	for f in "$log" "$log".*; do [ -f "$f" ] && logs=$((logs + $(bytes "$f"))); done
	jobs=$(ls "$ws/.itero/jobs" 2>/dev/null | wc -l | tr -d ' ')
	warm=$(warm_copies "$daemon")
	health='null'
	[ -n "$api" ] && health=$(curl -s --max-time 5 "$api/health" | jq -c '{passes, busy_since, connectors: (.connectors | map_values(.restarts))}' 2>/dev/null) || health='null'
	[ -n "$health" ] || health='null'
	echo "$lines" | jq -sc "$jq_defs"'
		map(select(. != null)) as $l
		| ($l | map(select(.event == "run.started"))) as $s
		| ($l | map(select(.event == "run.ended"))) as $e
		| {t: $t, phase: $phase, rss_kb: $rss, fds: $fds, warm: $warm, journal: $journal, wal: $wal, logs: $logs, jobs: $jobs,
			sent: {api: $api_sent, api_ok: $api_ok, lines: $lines_sent},
			started: ($s | group_by(.workflow) | map({key: .[0].workflow, value: length}) | from_entries),
			ended: ($e | group_by(.status) | map({key: .[0].status, value: length}) | from_entries),
			ended_by: ($e | group_by(.workflow) | map({key: .[0].workflow, value: length}) | from_entries),
			saved_to_start_ms: ($s | map(select(.message != null) | (.run | ms) - (.message | ms)) | pct),
			start_to_end_ms: ($e | map(((.time | secs) * 1000 | round) - (.run | ms)) | pct),
			space_full: ($l | map(select(.type == "itero.space.full")) | length),
			log_full: ($l | map(select(.type == "itero.log.full")) | length),
			health: $health}' \
		--argjson t "$t" --arg phase "$(cat "$ws/phase")" --argjson rss "$rss" --argjson fds "$fds" --argjson warm "$warm" \
		--argjson journal "$(bytes "$ws/.itero/journal.sqlite")" --argjson wal "$(bytes "$ws/.itero/journal.sqlite-wal")" \
		--argjson logs "$logs" --argjson jobs "$jobs" \
		--argjson api_sent "$(wc -l <"$ws/codes" | tr -d ' ')" --argjson api_ok "$(grep -c '^200' "$ws/codes" || :)" \
		--argjson lines_sent "$(cat "$ws/lines.sent")" --argjson health "$health" >>"$out/samples.jsonl"
}
sampling() {
	while :; do
		sleep "$every"
		sample || say "a sample failed"
	done
}
# runs of the soak's own workflows started and ended so far, from the
# samples (the schedule's and the child runs apart)
totals() {
	jq -s '{started: (map(.started | (.api // 0) + (.order // 0) + (.slow // 0) + (.warm // 0)) | add // 0),
		ended: (map(.ended_by | (.api // 0) + (.order // 0) + (.slow // 0) + (.warm // 0)) | add // 0),
		sent: (last | (.sent.api_ok + .sent.lines))}' "$out/samples.jsonl"
}

# --- the driver --------------------------------------------------------------
# one round a second, paced by the clock: that second's webhook requests
# through curl, its lines into the connector's fifo in one write
drive() { # seconds, burst from, burst to (seconds from the start)
	start=$(now)
	end=$((start + $1))
	next=$start
	sent=0
	n=0
	while [ "$next" -lt "$end" ]; do
		at=$((next - start))
		r=$rate
		[ "$at" -ge "$2" ] && [ "$at" -lt "$3" ] && r=$burst
		api_n=$((r * 5 / 10))
		order_n=$((r * 3 / 10))
		slow_n_=$((r / 10))
		warm_n=$((r - api_n - order_n - slow_n_))
		cfg="$ws/curl.$((n % 4))"
		awk -v n="$api_n" -v u="$hook/api" 'BEGIN { for (i = 0; i < n; i++) printf "url = \"%s\"\n", u }' >"$cfg"
		# spread over the second, 32 at once at most (what a hook takes:
		# past it, 503); what has not ended in 30 s is 000 and curl's 28
		curl -s --parallel --parallel-max 32 --rate "$((api_n > 0 ? api_n : 1))/s" --max-time 30 --config "$cfg" -X POST \
			-H "Authorization: Bearer $token" -H 'Content-Type: application/json' -d '{"wf":"api"}' \
			-o /dev/null -w '%{http_code} %{exitcode}\n' >>"$ws/codes" 2>/dev/null &
		awk -v o="$order_n" -v s="$slow_n_" -v w="$warm_n" 'BEGIN {
			for (i = 0; i < o; i++) print "{\"wf\":\"order\"}"
			for (i = 0; i < s; i++) print "{\"wf\":\"slow\"}"
			for (i = 0; i < w; i++) print "{\"wf\":\"warm\"}" }' >"$fifo"
		sent=$((sent + order_n + slow_n_ + warm_n))
		echo "$sent" >"$ws/lines.sent"
		n=$((n + 1))
		next=$((next + 1))
		while [ "$(now)" -lt "$next" ]; do sleep 0.05; done
	done
	wait
}

# waits until every event sent has started its run and every such run has
# ended, `max` seconds at most from `since`; prints the seconds from
# `since`, or null when it did not
wait_drained() { # max, events expected beyond those sent, since
	while :; do
		sample
		tt=$(totals | jq -r '"\(.started) \(.ended) \(.sent)"')
		started=${tt%% *} rest=${tt#* }
		ended=${rest%% *} sentn=${rest#* }
		if [ "$started" -ge "$((sentn + $2))" ] && [ "$ended" -ge "$started" ]; then
			echo $(($(now) - $3))
			return
		fi
		[ $(($(now) - $3)) -lt "$1" ] || {
			echo null
			return
		}
		sleep 1
	done
}

: >"$ws/codes"
echo 0 >"$ws/lines.sent"
echo 0 >"$ws/offset"
: >"$ws/inode"
: >"$out/samples.jsonl"
say "$mode, results in $out"
result --arg mode "$mode" --arg itero "$("$itero" --version)" --arg system "$(uname -sm), $(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu) CPUs" \
	--argjson rate "$rate" --argjson secs "$secs" --argjson burst "$burst" --argjson burst_secs "$burst_secs" \
	--argjson backlog "$backlog" --argjson limits "$limits" \
	'{test: "soak", mode: $mode, itero: $itero, system: $system, rate: $rate, secs: $secs,
		burst: $burst, burst_secs: $burst_secs, backlog: $backlog, limits: $limits}'

if [ "$mode" = slow ]; then
	for steps in $slow_steps; do
		jq --argjson s "$steps" '.limits.steps = $s' "$ws/itero.config.json" >"$ws/c.json" && mv "$ws/c.json" "$ws/itero.config.json"
		rm -rf "$ws/.itero"
		echo 0 >"$ws/offset"
		: >"$ws/inode"
		seq 1 "$slow_n" | (cd "$ws" && xargs -P 8 -I{} "$itero" ws emit soak.sent '{"wf":"slow"}' >/dev/null 2>>"$ws/emit.err") || {
			tail -3 "$ws/emit.err" >&2
			say "queuing failed"
			exit 1
		}
		echo "slow-$steps" >"$ws/phase"
		t0=$(now)
		start_daemon --no-serve
		until [ "$(grep -c '"event":"run.ended".*"workflow":"slow"' "$log" 2>/dev/null || :)" -ge "$slow_n" ]; do
			[ $(($(now) - t0)) -lt "$drain_max" ] || break
			sleep 0.5
		done
		stop_daemon
		jq -sc "$jq_defs"'
			(map(select(.event == "run.started" and .workflow == "slow")) | map(.time | secs) | min) as $s
			| map(select(.event == "run.ended" and .workflow == "slow")) as $e
			| ($e | map(.time | secs) | max) as $last
			| {test: "slow", steps: $steps, runs: ($e | length), of: $n, seconds: (($last - $s) * 1000 | round / 1000),
				runs_per_second: (($e | length) / ($last - $s) | round), ceiling: ($steps / 0.2 | round)}' \
			--argjson steps "$steps" --argjson n "$slow_n" "$log" | tee -a "$out/results.jsonl" >&2
	done
	say "done: $out"
	exit 0
fi

# --- full: sustained, with a burst halfway ------------------------------------
echo sustained >"$ws/phase"
start_daemon
sample
sampling &
sampler=$!
say "sustained: $rate a second for $secs s, $burst a second from $((secs / 2)) s for $burst_secs s"
drive "$secs" $((secs / 2)) $((secs / 2 + burst_secs)) &
driver=$!
# a write to the fifo waits while the connector is down, and a request
# 30 s at most: past 2 minutes over its time, the driver is stopped
deadline=$(($(now) + secs + 120))
while kill -0 "$driver" 2>/dev/null; do
	[ "$(now)" -lt "$deadline" ] || {
		say "the driver did not end in time: stopped"
		kill "$driver" 2>/dev/null || :
		break
	}
	sleep 1
done
wait "$driver" 2>/dev/null || :
driver=''
kill "$sampler" 2>/dev/null || :
wait "$sampler" 2>/dev/null || :
sampler=

# --- drain -------------------------------------------------------------------
echo drain >"$ws/phase"
say "drain: until everything sent has run, $drain_max s at most"
took=$(wait_drained "$drain_max" 0 "$(now)")
result --argjson took "$took" '{test: "drain", seconds: $took, timed_out: ($took == null)}'

# --- restart with a backlog -----------------------------------------------------
stop_daemon
echo restart >"$ws/phase"
say "restart: $backlog queued with the daemon stopped"
t0=$(now)
seq 1 "$backlog" | (cd "$ws" && xargs -P 8 -I{} "$itero" ws emit soak.sent '{"wf":"order"}' >/dev/null 2>>"$ws/emit.err") || {
	tail -3 "$ws/emit.err" >&2
	say "queuing failed"
	exit 1
}
queued=$(($(now) - t0))
before=$(totals | jq '.started')
t0=$(now)
start_daemon
up=$(($(now) - t0))
first=null
while [ $(($(now) - t0)) -lt "$drain_max" ]; do
	sample
	[ "$(totals | jq '.started')" -gt "$before" ] && {
		first=$(($(now) - t0))
		break
	}
	sleep 0.2
done
took=$(wait_drained "$drain_max" "$backlog" "$t0")
result --argjson queued "$queued" --argjson up "$up" --argjson first "$first" --argjson took "$took" --argjson n "$backlog" \
	'{test: "restart", backlog: $n, queue_seconds: $queued, start_seconds: $up, first_run_seconds: $first,
		drained_seconds: $took, timed_out: ($took == null)}'
stop_daemon

# --- summary -------------------------------------------------------------------
# the webhook's answers: HTTP code and curl's exit code (000 and 7: no
# connection; 000 and 28: no answer in 30 s)
sort "$ws/codes" | uniq -c | awk '{print "{\"code\":\"" $2 "\",\"curl\":" $3 ",\"count\":" $1 "}"}' |
	jq -sc '{test: "answers", answers: .}' | tee -a "$out/results.jsonl" >&2
jq -s '
	group_by(.phase) | map(
		(first) as $a | (last) as $b | (($b.t - $a.t) / 60) as $min
		| {key: $a.phase, value: {
			minutes: ($min * 10 | round / 10),
			started_per_second: ((map(.started | (.api // 0) + (.order // 0) + (.slow // 0) + (.warm // 0)) | add) / ([$b.t - $a.t, 1] | max) | round),
			ended: (map(.ended) | reduce .[] as $e ({}; reduce ($e | to_entries[]) as $kv (.; .[$kv.key] += $kv.value))),
			refused: ([$b.sent.api - $b.sent.api_ok, 0] | max),
			saved_to_start_p95_max: (map(.saved_to_start_ms.p95 // empty) | max),
			start_to_end_p95_max: (map(.start_to_end_ms.p95 // empty) | max),
			rss_kb: {first: $a.rss_kb, last: $b.rss_kb, max: (map(.rss_kb) | max)},
			fds: {first: $a.fds, last: $b.fds, max: (map(.fds) | max)},
			warm: (map(.warm // [] | .[]) | {copies: (group_by(.pid) | length),
				python_rss_kb_max: (map(.rss_kb) | max), worker_rss_kb_max: (map(.worker_rss_kb) | max),
				grew_kb_max: (group_by(.pid) | map(last.rss_kb - first.rss_kb) | max)}),
			journal: {first: $a.journal, last: $b.journal}, wal_max: (map(.wal) | max),
			logs: {first: $a.logs, last: $b.logs}, jobs: {first: $a.jobs, last: $b.jobs},
			space_full: (map(.space_full) | add), log_full: (map(.log_full) | add),
			restarts: ($b.health.connectors // null)}})
	| from_entries' "$out/samples.jsonl" >"$out/summary.json"
jq . "$out/summary.json" >&2
say "done: $out"
