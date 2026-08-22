import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
import content from './src/content.json' with { type: 'json' }

const topics = Object.keys(content.topics)
const pages = [
  ...topics.slice(1).map((topic) => ({ path: `/${topic}` })),
  ...topics.map((topic) => ({ path: topic === 'overview' ? '/ko' : `/ko/${topic}` })),
]

export default defineConfig({
  base: process.env.SITE_BASE_PATH || '/',
  plugins: [
    tanstackStart({
      prerender: {
        enabled: true,
        crawlLinks: false,
        failOnError: true,
      },
      pages,
    }),
    react(),
  ],
})
