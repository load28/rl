import { join } from 'node:path'

const root = join(import.meta.dir, '../dist/client')

Bun.serve({
  hostname: '127.0.0.1',
  port: Number(process.env.PORT ?? 4173),
  async fetch(request) {
    const url = new URL(request.url)
    let pathname = decodeURIComponent(url.pathname).replace(/^\/rl(?=\/|$)/, '')
    if (pathname.endsWith('/')) pathname += 'index.html'
    else if (!pathname.split('/').at(-1)?.includes('.')) pathname += '/index.html'

    const file = Bun.file(join(root, pathname))
    return await file.exists()
      ? new Response(file)
      : new Response('Not found', { status: 404 })
  },
})

console.log('Local preview: http://127.0.0.1:4173/rl/')
