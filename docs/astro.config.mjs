// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

const REPO = 'https://github.com/aegea-dev/itero';
// Served by GitHub Pages at https://aegea-dev.github.io/itero/. With a
// custom domain later, set SITE to it and BASE to '/'.
const SITE = 'https://aegea-dev.github.io';
const BASE = '/itero';

// Starlight prefixes BASE onto sidebar and asset links, but not onto links
// written in pages, so `[emit](/reference/commands/)` would leave the site.
// Pages keep writing root links and this adds the prefix, so a move to a
// domain of its own changes nothing but the two constants above.
function remarkBase() {
	const walk = (node) => {
		if (node.type === 'link' && /^\/(?!\/)/.test(node.url) && BASE !== '/') {
			node.url = BASE + node.url;
		}
		for (const child of node.children ?? []) walk(child);
	};
	return walk;
}

// https://astro.build/config
export default defineConfig({
	// Canonical URLs, the sitemap and the search index are built from these.
	site: SITE,
	base: BASE,
	trailingSlash: 'always',
	markdown: { remarkPlugins: [remarkBase] },
	integrations: [
		starlight({
			title: 'itero',
			description:
				'Runs workflows again and again, on a schedule or when an app sends an event, with the command line tools already on the machine.',
			expressiveCode: {
				themes: ['starlight-dark', 'github-light'],
				useStarlightDarkModeSwitch: true,
				useStarlightUiThemeColors: true,
			},
			favicon: '/favicon.svg',
			customCss: ['./src/styles/custom.css'],
			social: [{ icon: 'github', label: 'GitHub', href: REPO }],
			editLink: { baseUrl: `${REPO}/edit/main/docs/` },
			lastUpdated: true,
			// Search is Pagefind, built at compile time and served as static
			// assets: no account, no API key, nothing leaves the reader's browser.
			sidebar: [
				{
					label: 'Start here',
					items: [
						{ label: 'What itero is', slug: 'index' },
						{ label: 'Install', slug: 'start/install' },
						{ label: 'Quickstart', slug: 'start/quickstart' },
						{ label: 'Workspaces, workflows and runs', slug: 'start/concepts' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Writing a workflow', slug: 'guides/workflows' },
						{ label: 'Actions', slug: 'guides/actions' },
						{ label: 'Messages and sources', slug: 'guides/messages' },
						{ label: 'Args and secrets', slug: 'guides/secrets' },
						{ label: 'Start at login', slug: 'guides/service' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Glossary', slug: 'reference/glossary' },
						{ label: 'Commands', slug: 'reference/commands' },
						{ label: 'itero.toml', slug: 'reference/itero-toml' },
						{ label: 'Workflow files', slug: 'reference/workflow-file' },
						{ label: 'Actions', slug: 'reference/action-file' },
						{ label: 'HTTP API', slug: 'reference/http' },
					],
				},
			],
		}),
	],
});
