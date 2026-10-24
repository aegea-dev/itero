# itero docs

The source for <https://docs.itero.sh>, built with
[Astro](https://astro.build) and [Starlight](https://starlight.astro.build),
the same setup as primum's and videre's. `design.md` and the `plan-*.md`
files beside this README are the design and the iterations' plans; they are
not part of the site.

```bash
yarn install      # once
yarn dev          # http://localhost:4321
yarn build        # static site into dist/, then checks its local assets
yarn preview      # serve dist/ locally
yarn test         # the build check's own tests
```

Yarn 4, pinned by `packageManager` in `package.json`; `corepack enable` makes
that exact version run. `.yarnrc.yml` sets `nodeLinker: node-modules`: Yarn's
PnP linker breaks Astro's virtual modules, so keep it.

## Layout

```
src/content/docs/
  index.mdx          landing page
  start/             install, quickstart, concepts
  guides/            workflows, actions, messages, secrets, the log, the daemon's own workflows, the service
  commands/          an overview, and a page per command
  reference/         the glossary, file formats, the HTTP API
astro.config.mjs     sidebar, theme
```

Adding a page means creating the file and adding it to `sidebar` in
`astro.config.mjs`. A front matter value containing `: ` must be quoted.

## Deploying

`yarn build` writes plain static files to `dist/`, served by Cloudflare at
<https://docs.itero.sh>, as videre's docs are. `wrangler.jsonc` makes it an
assets-only Worker: no script runs on a request. Cloudflare builds from
`main` on every push; `.github/workflows/docs.yml` builds every pull request
that touches `docs/`.

Cloudflare project settings:

| Setting | Value |
|---|---|
| Root directory | `docs` |
| Build command | `corepack enable && yarn install --immutable && yarn build` |
| Deploy command | `npx wrangler deploy` (the default) |
| Node version | from `.node-version` |

`corepack enable` comes first because Cloudflare's build image ships Yarn 1,
which fails on this Yarn 4 lockfile. `--immutable` fails the build on a
`yarn.lock` that is out of date instead of resolving other versions.
