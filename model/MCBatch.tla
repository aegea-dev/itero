------------------------------- MODULE MCBatch -------------------------------
(* Two messages for a workflow whose batch keeps them for one run, full   *)
(* at a message or closed by its time; a cancel and a crash may come      *)
(* between any two steps.                                                  *)
EXTENDS Engine
BatchFlows == <<Keeping(<<Act>>, 1, {"k"}, FALSE)>>
=============================================================================
