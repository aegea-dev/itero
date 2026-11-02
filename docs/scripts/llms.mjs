// Writes llms.txt (llmstxt.org) for docs.itero.sh: the hand-written
// llms/intro.md, then the pages that matter most, each with its title and
// description from its front matter.
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const SITE = 'https://docs.itero.sh';

export const PAGES = [
	'start/concepts',
	'reference/concepts',
	'guides/workflows',
	'reference/workflow-file',
	'guides/actions',
	'reference/action-file',
	'guides/connectors',
	'guides/secrets',
	'reference/itero-config',
	'ai/workflows',
];

/** A page's title and description, from its front matter. */
export function frontMatter(text, slug) {
	const block = text.match(/^---\n([\s\S]*?)\n---/);
	if (!block) throw new Error(`${slug}: no front matter`);
	const field = (name) => {
		const line = block[1].split('\n').find((l) => l.startsWith(`${name}:`));
		const value = line?.slice(name.length + 1).trim().replace(/^"(.*)"$/, '$1');
		if (!value) throw new Error(`${slug}: no ${name} in its front matter`);
		return value;
	};
	return { title: field('title'), description: field('description') };
}

async function pageText(contentDir, slug) {
	for (const ext of ['.md', '.mdx']) {
		try {
			return await readFile(resolve(contentDir, `${slug}${ext}`), 'utf8');
		} catch (error) {
			if (error.code !== 'ENOENT') throw error;
		}
	}
	throw new Error(`${slug}: no page ${slug}.md or ${slug}.mdx`);
}

/** llms.txt's text: the intro, then `## Docs` with a line per page. */
export async function buildLlms({ intro, contentDir, pages = PAGES, site = SITE }) {
	const lines = [];
	for (const slug of pages) {
		const { title, description } = frontMatter(await pageText(contentDir, slug), slug);
		lines.push(`- [${title}](${site}/${slug}/): ${description}`);
	}
	return `${intro.trimEnd()}\n\n## Docs\n\n${lines.join('\n')}\n`;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
	const docs = fileURLToPath(new URL('../', import.meta.url));
	const intro = await readFile(resolve(docs, 'llms/intro.md'), 'utf8');
	const text = await buildLlms({ intro, contentDir: resolve(docs, 'src/content/docs') });
	const out = process.argv[2] ?? resolve(docs, 'dist/llms.txt');
	await writeFile(out, text);
	console.log(`Wrote ${out} with ${PAGES.length} pages.`);
}
