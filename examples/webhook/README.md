# webhook

Replies from your own app reach the step waiting for them. One workflow
takes every webhook and sends the events the others use.

    echo 'APP_TOKEN=a-long-random-value' > .env
    itero daemon

Then ask, and answer as your app would:

    itero ws emit app.ask '{"to": "+905550000000"}'
    curl -X POST http://127.0.0.1:8788/app \
      -H 'Authorization: Bearer a-long-random-value' \
      -H 'Content-Type: application/json' \
      -d '{"id": "m1", "from": "+905550000000", "text": "günaydın"}'

Then `itero ws wf runs` shows the `ask` run done, with the reply as its output.
