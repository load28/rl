import { createFileRoute, redirect } from '@tanstack/react-router'
import { ReferencePage, pageHead } from '../ui/ReferencePage'
import { isTopic, topicPath, type Language } from '../content'

type LegacySearch = { api?: string; lang?: Language }

export const Route = createFileRoute('/')({
  validateSearch: (search): LegacySearch => ({
    api: typeof search.api === 'string' ? search.api : undefined,
    lang: search.lang === 'ko' ? 'ko' : undefined,
  }),
  beforeLoad: ({ search }) => {
    if (search.lang || (search.api && isTopic(search.api))) {
      const topic = search.api && isTopic(search.api) ? search.api : 'overview'
      throw redirect({ to: topicPath(search.lang ?? 'en', topic) })
    }
  },
  head: () => pageHead('en', 'overview'),
  component: () => <ReferencePage language="en" topic="overview" />,
})
