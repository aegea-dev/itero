------------------------------ MODULE MCWindow ------------------------------
(* Two messages start an action each in two workflows; a window may hold  *)
(* either back from the first. A cancel and a crash may come between any  *)
(* two steps.                                                              *)
EXTENDS Engine
WindowFlows == <<Windowed(<<Act>>, 1, {"k"}, FALSE), Flow(<<Act>>, 1, {"k"}, FALSE)>>
=============================================================================
