import { Link } from '@tanstack/react-router'
import { useState } from 'react'
import {
  content,
  highlighted,
  topicIds,
  topicPath,
  type Language,
  type TopicId,
} from '../content'

export function pageHead(language: Language, topic: TopicId) {
  const item = content.topics[topic]
  const title = topic === 'overview'
    ? 'rl — TypeScript, with better control flow'
    : `${item.nav[language]} — rl`
  const description = item.summary[language]
  const origin = 'https://load28.github.io/rl'
  const canonical = `${origin}${topicPath(language, topic)}`
  return {
    meta: [
      { title },
      { name: 'description', content: description },
      { property: 'og:title', content: title },
      { property: 'og:description', content: description },
      { property: 'og:type', content: 'website' },
      { property: 'og:url', content: canonical },
    ],
    links: [
      { rel: 'canonical', href: canonical },
      { rel: 'alternate', hreflang: 'en', href: `${origin}${topicPath('en', topic)}` },
      { rel: 'alternate', hreflang: 'ko', href: `${origin}${topicPath('ko', topic)}` },
      { rel: 'alternate', hreflang: 'x-default', href: `${origin}${topicPath('en', topic)}` },
    ],
  }
}

export function ReferencePage({ language, topic }: { language: Language; topic: TopicId }) {
  const item = content.topics[topic]
  const group = content.groups.find(({ topics }) => topics.includes(topic))!
  const nextTopic = topicIds[topicIds.indexOf(topic) + 1]
  const otherLanguage: Language = language === 'ko' ? 'en' : 'ko'

  return (
    <div className="site-shell">
      <header className="topbar">
        <Link className="brand" to={topicPath(language, 'overview')} aria-label={language === 'ko' ? 'rl 홈' : 'rl home'}>
          <span className="brand__mark">rl</span>
          <span className="brand__tag">{language === 'ko' ? 'TypeScript를 위한 작은 언어' : 'a small language for TypeScript'}</span>
        </Link>
        <div className="topbar__actions">
          <Link className="language-toggle" to={topicPath(otherLanguage, topic)} aria-label={language === 'ko' ? '영어로 보기' : 'View in Korean'}>
            {language === 'ko' ? 'EN' : '한국어'}
          </Link>
          <a className="github-link" href="https://github.com/load28/rl" target="_blank" rel="noreferrer">GitHub ↗</a>
        </div>
      </header>

      <div className="reference-layout">
        <aside className="reference-nav" aria-label={language === 'ko' ? '언어 API 목록' : 'Language API list'}>
          {content.groups.map((navGroup) => (
            <section className="reference-nav__group" key={navGroup.id}>
              <h2 className="reference-nav__heading">{navGroup[language]}</h2>
              {(navGroup.topics as TopicId[]).map((id) => (
                <Link
                  className={`reference-nav__item${id === topic ? ' is-active' : ''}`}
                  to={topicPath(language, id)}
                  aria-current={id === topic ? 'page' : undefined}
                  key={id}
                >
                  {content.topics[id].nav[language]}
                </Link>
              ))}
            </section>
          ))}
        </aside>

        <main className="reference-content" id="content">
          <article className="reference-article">
            <p className="eyebrow">{group[language]}</p>
            <h1 className="reference-title">{item.title[language]}</h1>
            <p className="reference-summary">{item.summary[language]}</p>
            {topic === 'overview' && <InstallCommand language={language} />}

            <div className="code-block">
              <span className="code-block__label">{topic === 'cli' ? 'shell' : 'example.rl'}</span>
              <pre tabIndex={0} role="region" aria-label={language === 'ko' ? '코드 예제' : 'Code example'}>
                <code dangerouslySetInnerHTML={{ __html: highlighted[topic] }} />
              </pre>
            </div>

            <div className="detail-grid">
              <DetailList kind="works" title={language === 'ko' ? '지원 범위' : 'Supported'} items={item.works[language]} />
              <DetailList kind="limits" title={language === 'ko' ? '사용 시 주의사항' : 'Things to know'} items={item.limits[language]} />
            </div>

            {nextTopic && (
              <Link className="next-topic" to={topicPath(language, nextTopic)}>
                <span className="next-topic__label">{language === 'ko' ? '다음' : 'Next'}</span>
                <span className="next-topic__name">{content.topics[nextTopic].nav[language]} →</span>
              </Link>
            )}
          </article>
        </main>
      </div>
    </div>
  )
}

function InstallCommand({ language }: { language: Language }) {
  const command = 'npm i -D rl-lang typescript@7'
  const [copied, setCopied] = useState(false)
  return (
    <div className="install-command">
      <code className="install-command__code">{command}</code>
      <button
        className="install-command__copy"
        type="button"
        onClick={async () => {
          await navigator.clipboard.writeText(command)
          setCopied(true)
        }}
      >
        {copied ? (language === 'ko' ? '복사됨' : 'Copied') : (language === 'ko' ? '복사' : 'Copy')}
      </button>
    </div>
  )
}

function DetailList({ kind, title, items }: { kind: 'works' | 'limits'; title: string; items: string[] }) {
  return (
    <section className={`detail-list detail-list--${kind}`}>
      <h2 className="detail-list__title">{title}</h2>
      <ul className="detail-list__items">
        {items.map((item) => <li className="detail-list__item" key={item}>{item}</li>)}
      </ul>
    </section>
  )
}
