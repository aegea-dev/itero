import assert from 'node:assert/strict';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';

import { buildLlms, frontMatter, PAGES } from './llms.mjs';

async function content(pages) {
	const root = await mkdtemp(join(tmpdir(), 'itero-doc-llms-'));
	for (const [slug, text] of Object.entries(pages)) {
		const path = join(root, slug);
		await mkdir(dirname(path), { recursive: true });
		await writeFile(path, text);
	}
	return root;
}

const page = (title, description) => `---\ntitle: ${title}\ndescription: ${description}\n---\n\nGövde.\n`;

test('the intro comes first, then each page by its title, URL and description, in order', async () => {
	const root = await content({
		'start/çalışma.md': page('Çalışma alanı', 'Bir klasör.'),
		'ai/akış.mdx': page('"#ai in a workflow"', 'Bir model sorar.'),
	});
	const text = await buildLlms({
		intro: '# itero\n\n> Özet.\n',
		contentDir: root,
		pages: ['start/çalışma', 'ai/akış'],
		site: 'https://docs.example.test',
	});
	assert.equal(
		text,
		'# itero\n\n> Özet.\n\n## Docs\n\n' +
			'- [Çalışma alanı](https://docs.example.test/start/çalışma/): Bir klasör.\n' +
			'- [#ai in a workflow](https://docs.example.test/ai/akış/): Bir model sorar.\n',
	);
});

test('a page that is missing is an error naming it', async () => {
	const root = await content({});
	await assert.rejects(
		buildLlms({ intro: '# itero\n', contentDir: root, pages: ['guides/yok'] }),
		/guides\/yok: no page/,
	);
});

test('a page without a description is an error naming it', () => {
	assert.throws(() => frontMatter('---\ntitle: Başlık\n---\n', 'guides/eksik'), /guides\/eksik: no description/);
});

test('the list is the 10 pages agreed, in order', () => {
	assert.deepEqual(PAGES, [
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
	]);
});
