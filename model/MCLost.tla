------------------------------- MODULE MCLost -------------------------------
(* One workflow of one action; its job lost (killed, or its daemon died)   *)
(* until MAX_RUNS.                                                         *)
EXTENDS Engine
LostFlows == <<Flow(<<Act>>, 1, {"k"}, FALSE)>>
=============================================================================
