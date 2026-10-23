# languages

Actions in other languages, each in its subfolder of actions/, each
bringing its own packages with its own tools; itero installs nothing.

  python/slugify, python/parse-date   uv scripts (PEP 723): uv installs
                                       their packages on the first run
  node/links                           starts node/links-app, a Node project:
                                       (cd actions/node/links-app && npm install) once
  bun/check-order                      Bun installs zod on the first run
  docker/to-markdown                   pandoc in Docker (pandoc/minimal)

Run `itero daemon` in this folder, then from another terminal:
  itero emit --wait title.received '{"title": "Çok Güzel Bir Başlık"}'
  itero emit --wait reminder.requested '{"text": "yarın 15:00"}'
  itero emit --wait page.links '{"url": "https://www.rust-lang.org"}'
  itero emit --wait order.received '{"order": 43, "email": "ayse@example.com", "total": 120}'
  itero emit website.summarize '{"url": "https://www.rust-lang.org"}'
The last one also needs LM Studio with a model, and its token as
$$LM_STUDIO_TOKEN (see examples/page-summary); its summary shows in the
daemon's terminal. `itero check` says what a machine lacks.
