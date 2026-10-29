// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

const REPO = 'https://github.com/aegea-dev/itero';
const SITE = 'https://docs.itero.sh';

// https://astro.build/config
export default defineConfig({
	// Canonical URLs, the sitemap and the search index are built from it. Must
	// match the deployed origin, or sitemap and social links point at the wrong
	// host.
	site: SITE,
	trailingSlash: 'always',
	// pages moved on 2026-10-24 (the commands grouped by kind; "Messages and
	// connectors" became "Connectors"): their old addresses were published
	redirects: {
		'/commands/workflows/': '/commands/workflow/',
		'/commands/runs/': '/commands/workflow/',
		'/commands/errors/': '/commands/workflow/',
		'/commands/config/': '/commands/workflow/',
		'/commands/args/': '/commands/action/',
		'/guides/messages/': '/guides/connectors/',
		// the #ai guide became the itero-ai group (2026-10-29)
		'/guides/ai/': '/ai/workflows/',
	},
	integrations: [
		starlight({
			title: 'itero',
			// its colors follow light and dark mode inside the file
			logo: { src: './src/assets/itero.svg', alt: '' },
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
					label: 'Commands',
					items: [
						{ label: 'Overview', slug: 'commands' },
						{ label: 'daemon', slug: 'commands/daemon' },
						{ label: 'workflow ●', slug: 'commands/workflow' },
						{ label: 'action ■', slug: 'commands/action' },
						{ label: 'connector ▲', slug: 'commands/connector' },
						{ label: 'emit', slug: 'commands/emit' },
						{ label: 'check', slug: 'commands/check' },
						{ label: 'which', slug: 'commands/which' },
						{ label: 'info', slug: 'commands/info' },
						{ label: 'secret', slug: 'commands/secret' },
						{ label: 'ai', slug: 'commands/ai' },
						{ label: 'service', slug: 'commands/service' },
						{ label: 'watch', slug: 'commands/watch' },
						{ label: 'completion', slug: 'commands/completion' },
						{ label: 'help', slug: 'commands/help' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Writing your first workflow', slug: 'guides/first-workflow' },
						{ label: 'Workflows ●', slug: 'guides/workflows' },
						{ label: 'Actions ■', slug: 'guides/actions' },
						{ label: 'Connectors ▲', slug: 'guides/connectors' },
						{ label: 'Args and secrets', slug: 'guides/secrets' },
						{ label: 'The log, IDs and names', slug: 'guides/log' },
						{ label: "The daemon's own workflows", slug: 'guides/maintenance' },
						{ label: 'Start at login', slug: 'guides/service' },
					],
				},
				{
					label: 'itero-ai',
					items: [
						{ label: 'Overview', slug: 'ai' },
						{ label: 'Models', slug: 'ai/models' },
						{ label: 'Qwen3.5-4B', slug: 'ai/qwen3.5-4b' },
						{ label: '#ai in a workflow', slug: 'ai/workflows' },
						{ label: 'The API', slug: 'ai/api' },
						{ label: 'Running and troubleshooting', slug: 'ai/running' },
						{ label: 'Testing', slug: 'ai/testing' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Glossary', slug: 'reference/glossary' },
						{ label: 'itero.config.json', slug: 'reference/itero-config' },
						{ label: 'Workflow files', slug: 'reference/workflow-file' },
						{ label: 'Actions', slug: 'reference/action-file' },
						{ label: 'HTTP API', slug: 'reference/http' },
					],
				},
			],
		}),
	],
});
