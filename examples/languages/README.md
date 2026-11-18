# languages

Actions in other languages, each in its subfolder of actions/, each
bringing its own packages with its own tools; itero installs nothing.

  python/slugify, python/parse-date   uv scripts (PEP 723): uv installs
                                       their packages on the first run
  node/links                           starts node/links-app, a Node project:
                                       (cd actions/node/links-app && npm install) once
  bun/check-order                      Bun installs zod on the first run
  docker/to-markdown                   pandoc in Docker (pandoc/minimal)
  python/post-path, python/price       plain Python, no packages
  node/receipt                         Node, a single file, no packages
  sh/log-order                         a shell script

Two workflows of several steps, run warm where they are Python or Node (a
step's "warm": true keeps its action loaded between steps):
  post        three Python steps, all warm: slugify, parse-date, post-path
  order-flow  four languages: Bun checks the order, Python prices it
              (warm), Node writes a receipt line (warm), sh adds it to
              orders.log

Run `itero ws daemon` in this folder, then from another terminal:
  itero ws emit --wait title.received '{"title": "Çok Güzel Bir Başlık"}'
  itero ws emit --wait reminder.requested '{"text": "yarın 15:00"}'
  itero ws emit --wait page.links '{"url": "https://www.rust-lang.org"}'
  itero ws emit --wait order.received '{"order": 43, "email": "ayse@example.com", "total": 120}'
  itero ws emit --wait post.drafted '{"title": "Çok Güzel Bir Başlık", "when": "yarın 15:00"}'
  itero ws emit --wait order.paid '{"order": 43, "email": "ayse@example.com", "total": 120}'
  itero ws emit website.summarize '{"url": "https://www.rust-lang.org"}'
The last one also needs LM Studio with a model, and its token as
$$LM_STUDIO_TOKEN (see examples/page-summary); its summary shows in the
daemon's terminal. `itero ws check` says what a machine lacks.
