------------------------------- MODULE Engine -------------------------------
(***************************************************************************)
(* itero's engine as src/engine/ works it: one journal, worked by one      *)
(* daemon at a time, its managers and workers, commands from outside and   *)
(* crashes at any moment. Each journal transaction is one action, so a     *)
(* crash between any two writes is one more order TLC tries. README.md     *)
(* maps each action to the Rust it stands for.                             *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    Flows,           \* the workflows: a sequence of Flow(...), a run's wf its index
    Kinds,           \* the kinds of message from outside
    MaxEmits,        \* itero ws emit
    MaxStops,        \* itero ws wf stop
    MaxCancels,      \* itero ws ev cancel
    MaxCrashes,      \* the daemon dies, its workers with it (within 500 ms)
    MaxKills,        \* a worker is killed: its job is lost
    MaxManagerKills, \* a manager killed MAX_TRIES times: its event waits
    MaxRuns,         \* MAX_RUNS: how many times a job runs
    Limit,           \* limits.runs
    Bug              \* "none", or the code scripts/model.sh breaks

APP == 0      \* the subject itero.app: messages
WORKER == -1  \* the subject itero.worker: jobs; a run's subject is its id

\* A workflow's steps
Act == [kind |-> "act", wait |-> FALSE, flow |-> 0]
Wait == [kind |-> "act", wait |-> TRUE, flow |-> 0]      \* exits 75 until a message
Group == [kind |-> "group", wait |-> FALSE, flow |-> 0]  \* two actions side by side; the first ends it
Child(f) == [kind |-> "child", wait |-> FALSE, flow |-> f]

\* A workflow: its steps, its rounds (repeat), the kinds it starts on
\* ("run.done" for itero.run.done), whether it is a built-in.
Flow(steps, rounds, on, internal) ==
    [steps |-> steps, rounds |-> rounds, on |-> on, internal |-> internal]

VARIABLES
    ev,         \* table events, by seq; a job's runs (table jobs) and a ghost count ride on it
    runs,       \* table runs, by id
    timers,     \* table timers: the runs that have one (a waiting step's timeout)
    up,         \* a daemon holds daemon.lock
    taking,     \* the message the pass has read and routes next; 0 when none
    held,       \* the pass's memory: messages held for room under Limit,
    running,    \*   the jobs its workers run,
    cancel,     \*   those of them whose steps were stopped,
    deferred,   \*   and whether an event waits because its manager was killed
    orphans,    \* workers of a daemon that died, until they see it
    left,       \* what is left of each bounded thing
    overLimit,  \* ghost: a message started runs with Limit under way
    twice       \* ghost: a run took a message it had taken

vars == <<ev, runs, timers, up, taking, held, running, cancel, deferred,
          orphans, left, overLimit, twice>>

Ev(subj, type) ==
    [subj |-> subj, type |-> type, handled |-> FALSE, note |-> "", marks |-> 0,
     kind |-> "", wf |-> 0, run |-> 0, path |-> 0, br |-> 0, msg |-> 0,
     cause |-> 0, tries |-> 0]

Run(wf, parent, ppath, round, msg, from, origin) ==
    [wf |-> wf, pos |-> 0, open |-> {}, waiting |-> FALSE, child |-> FALSE,
     ended |-> "", parent |-> parent, ppath |-> ppath, round |-> round,
     msg |-> msg, from |-> from, taken |-> {}, origin |-> origin]

Mark(e, note) == [e EXCEPT !.handled = TRUE, !.note = note, !.marks = @ + 1]
Map(s, F(_)) == [k \in 1..Len(s) |-> F(s[k])]
Ids == [j \in 1..Len(ev) |-> j]
Step(w, k) == Flows[w].steps[k]
ActiveOf(rs) ==
    Cardinality({r \in DOMAIN rs : rs[r].ended = "" /\ ~Flows[rs[r].wf].internal})

Init ==
    /\ ev = <<>>
    /\ runs = <<>>
    /\ timers = {}
    /\ up = TRUE
    /\ taking = 0
    /\ held = {} /\ running = {} /\ cancel = {} /\ deferred = FALSE
    /\ orphans = {}
    /\ left = [emit |-> MaxEmits, stop |-> MaxStops, cancel |-> MaxCancels,
               crash |-> MaxCrashes, kill |-> MaxKills, mkill |-> MaxManagerKills]
    /\ overLimit = FALSE /\ twice = FALSE

-----------------------------------------------------------------------------
(* From outside, at any moment *)

\* itero ws emit
Emit(k) ==
    /\ left.emit > 0
    /\ ev' = Append(ev, [Ev(APP, "outside") EXCEPT !.kind = k])
    /\ left' = [left EXCEPT !.emit = @ - 1]
    /\ UNCHANGED <<runs, timers, up, taking, held, running, cancel, deferred,
                   orphans, overLimit, twice>>

\* journal.rs cancel: a message no daemon has handled
CancelEvent(j) ==
    /\ left.cancel > 0
    /\ ev[j].subj = APP /\ ~ev[j].handled
    /\ ev' = [ev EXCEPT ![j] = Mark(@, "cancelled")]
    /\ left' = [left EXCEPT !.cancel = @ - 1]
    /\ UNCHANGED <<runs, timers, up, taking, held, running, cancel, deferred,
                   orphans, overLimit, twice>>

\* events.rs stop_run: a run.stop event, while the run has not ended
StopRun(r) ==
    /\ left.stop > 0
    /\ runs[r].ended = ""
    /\ ev' = Append(ev, Ev(r, "run.stop"))
    /\ left' = [left EXCEPT !.stop = @ - 1]
    /\ UNCHANGED <<runs, timers, up, taking, held, running, cancel, deferred,
                   orphans, overLimit, twice>>

-----------------------------------------------------------------------------
(* The daemon's workers: daemon.rs work, end_one, save_result *)

Open(j) == ~ev[j].handled /\ ev[j].subj /= WORKER /\ j \notin held
NoEvent == \A j \in DOMAIN ev : ~Open(j)

StepOf(s) == Step(runs[ev[s].run].wf, ev[s].path)
Outcomes(s) ==
    IF StepOf(s).kind = "group" THEN {"finished"}
    ELSE IF StepOf(s).wait THEN {"waiting", "finished"}
    ELSE {"finished", "failed"}

\* save_result: the job handled, its result the run's next event
Save(s, o) ==
    ev' = Append([ev EXCEPT ![s] = Mark(@, "")],
                 [Ev(ev[s].run, "action." \o o) EXCEPT !.path = ev[s].path,
                      !.br = ev[s].br, !.cause = s])

\* work: once no event is left, each job not running is counted, then
\* started; one counted MAX_RUNS times already is an error instead
StartJob(s) ==
    /\ up /\ taking = 0 /\ ~deferred /\ NoEvent
    /\ ev[s].subj = WORKER /\ ~ev[s].handled /\ s \notin running
    /\ IF ev[s].tries >= MaxRuns /\ Bug /= "maxruns"
       THEN Save(s, "failed") /\ UNCHANGED running
       ELSE /\ ev' = [ev EXCEPT ![s].tries = @ + 1]
            /\ running' = running \cup {s}
    /\ UNCHANGED <<runs, timers, up, taking, held, cancel, deferred, orphans,
                   left, overLimit, twice>>

\* end_one: a worker ends; a stopped one's answer is dropped
EndWorker(s, o) ==
    /\ up /\ taking = 0 /\ s \in running /\ o \in Outcomes(s)
    /\ running' = running \ {s}
    /\ cancel' = cancel \ {s}
    /\ IF s \in cancel /\ Bug /= "cancel" THEN UNCHANGED ev ELSE Save(s, o)
    /\ UNCHANGED <<runs, timers, up, taking, held, deferred, orphans, left,
                   overLimit, twice>>

\* a worker killed: lost (lose_job), to run again, or after MAX_RUNS an error
KillWorker(s) ==
    /\ up /\ taking = 0 /\ s \in running /\ left.kill > 0
    /\ left' = [left EXCEPT !.kill = @ - 1]
    /\ running' = running \ {s}
    /\ cancel' = cancel \ {s}
    /\ IF s \in cancel \/ ev[s].tries < MaxRuns THEN UNCHANGED ev
       ELSE Save(s, "failed")
    /\ UNCHANGED <<runs, timers, up, taking, held, deferred, orphans,
                   overLimit, twice>>

\* the daemon dies: its memory is gone; its workers see it within 500 ms
Crash ==
    /\ up /\ left.crash > 0
    /\ left' = [left EXCEPT !.crash = @ - 1]
    /\ up' = FALSE
    /\ taking' = 0 /\ held' = {} /\ running' = {} /\ cancel' = {} /\ deferred' = FALSE
    /\ orphans' = orphans \cup running
    /\ UNCHANGED <<ev, runs, timers, overLimit, twice>>

\* worker.rs stop_with_parent: an orphan sees its parent gone and dies
OrphanDies(s) ==
    /\ s \in orphans
    /\ orphans' = orphans \ {s}
    /\ UNCHANGED <<ev, runs, timers, up, taking, held, running, cancel,
                   deferred, left, overLimit, twice>>

\* a daemon takes the lock and starts its pass from the journal
Restart ==
    /\ ~up
    /\ up' = TRUE
    /\ UNCHANGED <<ev, runs, timers, taking, held, running, cancel, deferred,
                   orphans, left, overLimit, twice>>

-----------------------------------------------------------------------------

Next ==
    \/ \E k \in Kinds : Emit(k)
    \/ \E j \in DOMAIN ev : CancelEvent(j)
    \/ \E r \in DOMAIN runs : StopRun(r)
    \/ \E s \in DOMAIN ev : \/ StartJob(s) \/ KillWorker(s) \/ OrphanDies(s)
                            \/ \E o \in {"finished", "failed", "waiting"} : EndWorker(s, o)
    \/ Crash \/ Restart

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Rules *)

\* 1. An event is handled at most once.
HandledOnce == \A j \in DOMAIN ev : ev[j].marks <= 1
=============================================================================
