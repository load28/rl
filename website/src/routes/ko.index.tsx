import { createFileRoute } from '@tanstack/react-router'
import { ReferencePage, pageHead } from '../ui/ReferencePage'

export const Route = createFileRoute('/ko/')({
  head: () => pageHead('ko', 'overview'),
  component: () => <ReferencePage language="ko" topic="overview" />,
})
