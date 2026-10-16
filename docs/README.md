# itero docs

The source for itero's documentation site, built with
[Astro](https://astro.build) and [Starlight](https://starlight.astro.build),
the same setup as primum's and videre's. `design.md` and the `plan-*.md`
files beside this README are the design and the iterations' plans; they are
not part of the site.

```bash
yarn install      # once
yarn dev          # http://localhost:4321/itero/
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
  guides/            workflows, actions, triggers, apps, secrets, the service
  reference/         commands, file formats, the HTTP API
astro.config.mjs     sidebar, theme
```

Adding a page means creating the file and adding it to `sidebar` in
`astro.config.mjs`. A front matter value containing `: ` must be quoted.

## Deploying

The site is served by GitHub Pages at <https://aegea-dev.github.io/itero/>.
`.github/workflows/docs.yml` builds it on every pull request that touches
`docs/`, and publishes it from `main`. The repository's Pages setting is
"GitHub Actions".

Under Pages the site lives at `/itero/`: `base` in `astro.config.mjs` says
so, and a small remark plugin there prefixes it onto root links written in
pages, so pages keep writing `[commands](/reference/commands/)`. Links in
front matter (the landing page's hero) are relative instead. To move to a
domain of its own, set `SITE` and `BASE` in `astro.config.mjs`, the base
argument of the build check in `package.json`, and the `Sitemap:` line in
`public/robots.txt`.
