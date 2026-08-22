import { readFile, writeFile } from 'node:fs/promises'
import { createHighlighter } from 'shiki'
import content from '../src/content.json'

const grammarUrl = new URL('../../editors/vscode/syntaxes/rl.tmLanguage.json', import.meta.url)
const grammar = JSON.parse(await readFile(grammarUrl, 'utf8'))
const highlighter = await createHighlighter({
  langs: [grammar, 'shellscript'],
  themes: ['github-dark-default'],
})

const highlighted = Object.fromEntries(
  Object.entries(content.topics).map(([id, topic]) => [
    id,
    highlighter.codeToHtml(topic.code, {
      lang: id === 'cli' ? 'shellscript' : 'rl',
      theme: 'github-dark-default',
      structure: 'inline',
    }),
  ]),
)

await writeFile(new URL('../src/highlighted.json', import.meta.url), `${JSON.stringify(highlighted, null, 2)}\n`)

const origin = 'https://load28.github.io/rl'
const paths = Object.keys(content.topics).flatMap((topic) => [
  topic === 'overview' ? '/' : `/${topic}`,
  topic === 'overview' ? '/ko' : `/ko/${topic}`,
])
const sitemap = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${paths.map((path) => `  <url><loc>${origin}${path}</loc></url>`).join('\n')}
</urlset>
`
await writeFile(new URL('../public/sitemap.xml', import.meta.url), sitemap)
