------------------------------- MODULE MCHeld -------------------------------
(* A workflow that waits for a message, started twice with Limit 1: the    *)
(* second message is held until the first run ends; a cancel and a crash  *)
(* may come between.                                                       *)
EXTENDS Engine
HeldFlows == <<Flow(<<Wait>>, 1, {"k"}, FALSE)>>
=============================================================================
