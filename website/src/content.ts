import rawContent from './content.json'
import rawHighlighted from './highlighted.json'

export type Language = 'en' | 'ko'
export type TopicId = keyof typeof rawContent.topics

export const content = rawContent
export const highlighted = rawHighlighted as Record<TopicId, string>
export const topicIds = Object.keys(content.topics) as TopicId[]

export function isTopic(value: string): value is TopicId {
  return value in content.topics
}

export function topicPath(language: Language, topic: TopicId) {
  if (language === 'ko') return topic === 'overview' ? '/ko' : `/ko/${topic}`
  return topic === 'overview' ? '/' : `/${topic}`
}
