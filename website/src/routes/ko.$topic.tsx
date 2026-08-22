import { createFileRoute, notFound } from '@tanstack/react-router'
import { isTopic } from '../content'
import { ReferencePage, pageHead } from '../ui/ReferencePage'

export const Route = createFileRoute('/ko/$topic')({
  beforeLoad: ({ params }) => {
    if (!isTopic(params.topic) || params.topic === 'overview') throw notFound()
  },
  head: ({ params }) => isTopic(params.topic) ? pageHead('ko', params.topic) : {},
  component: TopicPage,
})

function TopicPage() {
  const { topic } = Route.useParams()
  if (!isTopic(topic)) return null
  return <ReferencePage language="ko" topic={topic} />
}
