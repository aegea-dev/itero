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
(* The manager: steps.rs decide, as each workflow's steps. Its answer: the *)
(* run, the jobs to give out, a child to start, the leaves it stopped.     *)

Answer(run, jobs, child, stopped) ==
    [run |-> run, jobs |-> jobs, child |-> child, stopped |-> stopped]
Same(run) == Answer(run, <<>>, 0, {})
End(run, how) ==
    [run EXCEPT !.open = {}, !.waiting = FALSE, !.child = FALSE, !.ended = how]

\* the run at step k, just started
Begin(run, k) ==
    LET s == Step(run.wf, k)
        at == [run EXCEPT !.pos = k, !.waiting = FALSE, !.child = (s.kind = "child"),
                   !.open = CASE s.kind = "act" -> {1}
                              [] s.kind = "group" -> {1, 2}
                              [] OTHER -> {}]
    IN Answer(at,
              CASE s.kind = "act" -> <<<<k, 1>>>>
                [] s.kind = "group" -> <<<<k, 1>>, <<k, 2>>>>
                [] OTHER -> <<>>,
              IF s.kind = "child" THEN s.flow ELSE 0,
              {})

\* the step at run.pos ended well: the next, or the run is done
Advance(run) ==
    IF run.pos < Len(Flows[run.wf].steps) THEN Begin(run, run.pos + 1)
    ELSE Same(End(run, "done"))

Here(run, e) == e.path = run.pos /\ e.br \in run.open
Others(run, e) == {<<run.pos, b>> : b \in run.open \ {e.br}}

Decide(run, e) ==
    IF run.ended /= "" /\ Bug /= "ended" THEN Same(run)
    ELSE CASE e.type = "run.started" ->
                IF run.pos = 0 THEN Begin(run, 1) ELSE Same(run)
           [] e.type = "action.finished" ->
                IF Here(run, e) THEN [Advance(run) EXCEPT !.stopped = Others(run, e)]
                ELSE Same(run)
           [] e.type = "action.failed" ->
                IF Here(run, e) THEN Answer(End(run, "error"), <<>>, 0, Others(run, e))
                ELSE Same(run)
           [] e.type = "action.waiting" ->
                IF Here(run, e) THEN Same([run EXCEPT !.waiting = TRUE]) ELSE Same(run)
           [] e.type = "message" ->
                \* taken by the waiting step, which runs again; it still waits
                IF ~run.waiting \/ (e.msg \in run.taken /\ Bug /= "taken") THEN Same(run)
                ELSE Answer([run EXCEPT !.taken = @ \cup {e.msg}], <<<<run.pos, 1>>>>, 0, {})
           [] e.type = "workflow.ended" ->
                IF e.path = run.pos /\ run.child THEN Advance([run EXCEPT !.child = FALSE])
                ELSE Same(run)
           [] e.type = "timer.fired" ->
                IF run.waiting THEN Same(End(run, "error")) ELSE Same(run)
           [] e.type = "run.stop" ->
                Answer(End(run, "stopped"), <<>>, 0, {<<run.pos, b>> : b \in run.open})
           [] OTHER -> Same(run)

-----------------------------------------------------------------------------
(* The daemon's pass: daemon.rs work, route, deliver; journal.rs handle *)

First(j) == Open(j) /\ \A k \in 1..(j - 1) : ~Open(k)
Deliverable(j) == IF Bug = "order" THEN Open(j) ELSE First(j)

\* the oldest event is a message: read it (route's transaction is next)
Take(j) ==
    /\ up /\ taking = 0 /\ Deliverable(j) /\ ev[j].subj = APP
    /\ taking' = j
    /\ UNCHANGED <<ev, runs, timers, up, held, running, cancel, deferred,
                   orphans, left, overLimit, twice>>

Starts(e) ==
    SelectSeq([w \in 1..Len(Flows) |-> w],
              LAMBDA w : e.kind \in Flows[w].on /\ ~(e.type = "itero.run.done" /\ e.wf = w))
Counted(e) == \E k \in 1..Len(Starts(e)) : ~Flows[Starts(e)[k]].internal

\* journal.rs route: handled first (one cancelled since it was read starts
\* nothing), its runs started, copied to the runs waiting
Route ==
    /\ up /\ taking /= 0
    /\ LET j == taking
           e == ev[j]
           s == Starts(e)
           full == Counted(e) /\ ActiveOf(runs) >= Limit
           waiters == SelectSeq([r \in 1..Len(runs) |-> r],
                                LAMBDA r : runs[r].waiting /\ runs[r].ended = "")
           reach == IF e.type = "itero.run.done" THEN <<>> ELSE waiters
           started == [k \in 1..Len(s) |->
                        [Ev(Len(runs) + k, "run.started") EXCEPT !.cause = j]]
           copies == Map(reach, LAMBDA r : [Ev(r, "message") EXCEPT !.msg = j])
           fresh == [k \in 1..Len(s) |-> Run(s[k], 0, 0, 1, j, Len(ev) + k, e.type)]
       IN IF e.handled /\ Bug /= "handled"
          THEN UNCHANGED <<ev, runs, held, overLimit>>
          ELSE IF full /\ Bug /= "limit"
          THEN /\ held' = held \cup {j}
               /\ UNCHANGED <<ev, runs, overLimit>>
          ELSE /\ ev' = [ev EXCEPT ![j] = Mark(@, @.note)] \o started \o copies
               /\ runs' = runs \o fresh
               /\ overLimit' = (overLimit \/ full)
               /\ UNCHANGED held
    /\ taking' = 0
    /\ UNCHANGED <<timers, up, running, cancel, deferred, orphans, left, twice>>

\* deliver: a run's event to its manager, the answer saved in one handle
Deliver(j) ==
    /\ up /\ taking = 0 /\ Deliverable(j) /\ ev[j].subj > 0
    /\ LET e == ev[j]
           r == e.subj
           old == runs[r]
           a == Decide(old, e)
           new == a.run
           ends == new.ended /= "" /\ old.ended = ""
           again == ends /\ new.ended = "done" /\ old.round < Flows[old.wf].rounds
           jobs == Map(a.jobs, LAMBDA p : [Ev(WORKER, "action.requested") EXCEPT
                               !.run = r, !.path = p[1], !.br = p[2], !.cause = j])
           \* journal.rs messages_since: each message for the workspace
           \* since the run started, but the one that started it
           missed == IF new.waiting /\ ~old.waiting
                     THEN SelectSeq(Ids, LAMBDA m : ev[m].subj = APP /\ ev[m].type = "outside"
                                                   /\ m >= old.from /\ m /= old.msg)
                     ELSE <<>>
           copies == Map(missed, LAMBDA m : [Ev(r, "message") EXCEPT !.msg = m])
           \* quiet_when_done: one a run's message started says nothing
           done == IF ends /\ new.ended = "done"
                        /\ (old.origin /= "itero.run.done" \/ Bug = "quiet")
                   THEN <<[Ev(APP, "itero.run.done") EXCEPT !.kind = "run.done",
                               !.wf = old.wf, !.run = r]>>
                   ELSE <<>>
           \* a workflow used as a step: its end goes back, after its last round
           back == IF ends /\ old.parent /= 0 /\ (~again \/ Bug = "rounds") /\ Bug /= "back"
                   THEN <<[Ev(old.parent, "workflow.ended") EXCEPT !.path = old.ppath,
                               !.run = r, !.cause = j]>>
                   ELSE <<>>
           stopped == {s \in DOMAIN ev : ev[s].subj = WORKER /\ ~ev[s].handled
                                         /\ ev[s].run = r /\ <<ev[s].path, ev[s].br>> \in a.stopped}
           why == IF ends /\ new.ended = "stopped" THEN "run stopped" ELSE "stopped"
           marked == [s \in DOMAIN ev |-> IF s = j THEN Mark(ev[s], "")
                                          ELSE IF s \in stopped THEN Mark(ev[s], why)
                                          ELSE ev[s]]
           head == marked \o jobs \o copies \o done \o back
           round == IF again THEN <<[Ev(Len(runs) + 1, "run.started") EXCEPT !.cause = j]>>
                    ELSE <<>>
           nextRun == Run(old.wf, old.parent, old.ppath, old.round + 1, old.msg,
                          Len(head) + 1, old.origin)
           kid == IF a.child /= 0
                  THEN <<[Ev(Len(runs) + Len(round) + 1, "run.started") EXCEPT !.cause = j]>>
                  ELSE <<>>
           kidRun == Run(a.child, r, new.pos, 1, 0, Len(head) + Len(round) + 1, old.origin)
           newRuns == [runs EXCEPT ![r] = new]
                        \o (IF again THEN <<nextRun>> ELSE <<>>)
                        \o (IF a.child /= 0 THEN <<kidRun>> ELSE <<>>)
       IN /\ ev' = head \o round \o kid
          /\ runs' = newRuns
          /\ timers' = IF new.waiting /\ new.ended = "" THEN timers \cup {r}
                       ELSE IF new.ended /= "" /\ Bug = "timer" THEN timers
                       ELSE timers \ {r}
          /\ cancel' = cancel \cup (stopped \cap running)
          /\ held' = IF held /= {} /\ ActiveOf(newRuns) < Limit /\ Bug /= "held"
                     THEN {} ELSE held
          /\ twice' = (twice \/ (e.type = "message" /\ e.msg \in old.taken /\ a.jobs /= <<>>))
    /\ deferred' = FALSE
    /\ UNCHANGED <<up, taking, running, orphans, left, overLimit>>

\* ask_again: the manager killed MAX_TRIES times; the event is given again
\* in a second, and no job starts meanwhile
ManagerKilled(j) ==
    /\ up /\ taking = 0 /\ Deliverable(j) /\ ev[j].subj > 0 /\ left.mkill > 0
    /\ left' = [left EXCEPT !.mkill = @ - 1]
    /\ deferred' = TRUE
    /\ UNCHANGED <<ev, runs, timers, up, taking, held, running, cancel,
                   orphans, overLimit, twice>>

\* fire_timers: a due timer becomes its run's event, in one transaction
FireTimer(r) ==
    /\ up /\ taking = 0 /\ r \in timers
    /\ timers' = timers \ {r}
    /\ ev' = Append(ev, Ev(r, "timer.fired"))
    /\ UNCHANGED <<runs, up, taking, held, running, cancel, deferred, orphans,
                   left, overLimit, twice>>

-----------------------------------------------------------------------------

Outs == {"finished", "failed", "waiting"}

\* what the daemon does by itself: it goes on while it can
Daemon ==
    \/ \E j \in DOMAIN ev : Take(j) \/ Deliver(j)
    \/ Route
    \/ \E s \in DOMAIN ev : StartJob(s) \/ OrphanDies(s) \/ \E o \in Outs : EndWorker(s, o)
    \/ \E r \in DOMAIN runs : FireTimer(r)
    \/ Restart

\* Daemon's actions listed again, one a line: a trace names a step by its
\* action, or by its line here when the action ranges over the state
Next ==
    \/ \E j \in DOMAIN ev : Take(j)
    \/ \E j \in DOMAIN ev : Deliver(j)
    \/ Route
    \/ \E s \in DOMAIN ev : StartJob(s)
    \/ \E s \in DOMAIN ev : OrphanDies(s)
    \/ \E s \in DOMAIN ev : \E o \in Outs : EndWorker(s, o)
    \/ \E r \in DOMAIN runs : FireTimer(r)
    \/ Restart
    \/ \E k \in Kinds : Emit(k)
    \/ \E j \in DOMAIN ev : CancelEvent(j)
    \/ \E j \in DOMAIN ev : ManagerKilled(j)
    \/ \E r \in DOMAIN runs : StopRun(r)
    \/ \E s \in DOMAIN ev : KillWorker(s)
    \/ Crash

Spec == Init /\ [][Next]_vars /\ WF_vars(Daemon)

-----------------------------------------------------------------------------
(* Rules *)

\* 1. An event is handled at most once.
HandledOnce == \A j \in DOMAIN ev : ev[j].marks <= 1

\* 2. A cancelled message starts no run and reaches none.
CancelledReachesNone ==
    \A j \in DOMAIN ev : ev[j].note = "cancelled" =>
        /\ \A r \in DOMAIN runs : runs[r].msg /= j
        /\ \A k \in DOMAIN ev : ~(ev[k].type = "message" /\ ev[k].msg = j)

\* 3. A run that ended never changes again.
EndedStays == [][\A r \in DOMAIN runs : runs[r].ended /= "" => runs'[r] = runs[r]]_vars

\* 4. A job runs at most MAX_RUNS times; a stopped job's result never
\* reaches its run.
JobRunsBounded == \A j \in DOMAIN ev : ev[j].tries <= MaxRuns
StoppedSaysNothing ==
    \A j \in DOMAIN ev :
        ev[j].type \in {"action.finished", "action.failed", "action.waiting"}
            => ev[ev[j].cause].note \notin {"stopped", "run stopped"}

\* 5. At most one copy of a job runs at a time.
OneCopy == running \cap orphans = {}

\* 6. A message starts runs only while fewer than Limit are under way.
LimitKept == ~overLimit

\* 7. A run takes each message at most once.
TakenOnce == ~twice

\* 8. A run's events reach its manager in the order they were written.
InOrder ==
    \A j, k \in DOMAIN ev :
        (j < k /\ ev[j].subj = ev[k].subj /\ ev[j].subj > 0 /\ ev[k].handled) => ev[j].handled

\* 9. A parent gets one workflow.ended per child step.
OneEnd ==
    \A j, k \in DOMAIN ev :
        (j /= k /\ ev[j].type = "workflow.ended" /\ ev[k].type = "workflow.ended"
         /\ ev[j].subj = ev[k].subj) => ev[j].path /= ev[k].path

\* 10. An ended run has no timers.
EndedNoTimer == \A r \in timers : runs[r].ended = ""

\* 11. Every event is handled.
AllHandled == <>[](\A j \in DOMAIN ev : ev[j].handled)

\* 12. Every child's end reaches its parent, unless the parent has ended.
LastRound(c) == ~(runs[c].ended = "done" /\ runs[c].round < Flows[runs[c].wf].rounds)
ChildrenReport ==
    <>[](\A c \in DOMAIN runs :
            (runs[c].parent /= 0 /\ runs[c].ended /= "" /\ LastRound(c))
                => \/ \E j \in DOMAIN ev : ev[j].type = "workflow.ended" /\ ev[j].run = c
                   \/ runs[runs[c].parent].ended /= "")

\* 13. Every run ends, or waits.
RunsEnd == <>[](\A r \in DOMAIN runs : runs[r].ended /= "" \/ runs[r].waiting)

\* 14. A run whose chain started on itero.run.done says nothing when done.
NoChain ==
    \A j \in DOMAIN ev :
        ev[j].type = "itero.run.done" => runs[ev[j].run].origin /= "itero.run.done"
=============================================================================
