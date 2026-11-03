------------------------------- MODULE MCStop -------------------------------
(* One message starts an action and a group of two side by side; a stop,  *)
(* a cancel, a crash and a lost job may come between any two steps.       *)
EXTENDS Engine
StopFlows == <<Flow(<<Act>>, 1, {"k"}, FALSE), Flow(<<Group>>, 1, {"k"}, FALSE)>>
=============================================================================
