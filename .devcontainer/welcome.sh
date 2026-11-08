#!/bin/sh
# Said each time a terminal attaches.
cat <<TEXT
$(itero --version 2>/dev/null || echo "itero is still installing: see the creation log"), with its examples in examples/.

  cd examples/<name>      each example's README says what it does and how
  itero daemon            runs it; Ctrl-C stops it (a sleeping container stops it too)
  itero ws emit ...       in another terminal: an event for it

Notifications go to $ITERO_NOTIFY_LOG. The webhook examples listen on
port 8788, which has a public address: see the Ports tab.
TEXT
