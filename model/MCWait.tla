------------------------------- MODULE MCWait -------------------------------
(* A workflow whose two steps each wait for a message: "k" starts it, "m"  *)
(* is what it waits for. Messages are routed to it while it waits, and     *)
(* copied when it starts to wait; a stop and a cancel may come between.    *)
EXTENDS Engine
WaitFlows == <<Flow(<<Wait, Wait>>, 1, {"k"}, FALSE)>>
=============================================================================
