------------------------------- MODULE MCChild ------------------------------
(* A workflow whose first step is another workflow of 2 rounds; a built-in *)
(* starts on itero.run.done, and must not start a chain of its own.        *)
EXTENDS Engine
ChildFlows == <<Flow(<<Child(2), Act>>, 1, {"k"}, FALSE),
                Flow(<<Act>>, 2, {}, FALSE),
                Flow(<<Act>>, 1, {"run.done"}, TRUE)>>
=============================================================================
