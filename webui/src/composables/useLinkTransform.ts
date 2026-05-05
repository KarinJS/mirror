import { ref, computed } from 'vue'

export type RuleLabel = 'githubReleases' | 'githubRaw' | 'githubAvatar' | 'npmUnpkg'

export interface TransformRule {
  pattern: RegExp
  transform: (m: RegExpMatchArray) => string
  label: RuleLabel
  color: string
}

export interface TransformResult {
  label: RuleLabel
  color: string
  url: string
  source: string
}

const ORIGIN = typeof window !== 'undefined' ? window.location.origin : ''

const rules: TransformRule[] = [
  {
    label: 'githubReleases', color: '#7dd3fc',
    pattern: /^https?:\/\/github\.com\/([^/]+)\/([^/]+)\/releases\/download\/([^/]+)\/(.+)$/,
    transform: (m) => `${ORIGIN}/gh/${m[1]}/${m[2]}/releases/download/${m[3]}/${m[4]}`,
  },
  {
    label: 'githubRaw', color: '#86efac',
    pattern: /^https?:\/\/raw\.githubusercontent\.com\/([^/]+)\/([^/]+)\/([^/]+)\/(.+)$/,
    transform: (m) => `${ORIGIN}/raw/${m[1]}/${m[2]}/${m[3]}/${m[4]}`,
  },
  {
    label: 'githubRaw', color: '#86efac',
    pattern: /^https?:\/\/github\.com\/([^/]+)\/([^/]+)\/(?:raw|blob)\/([^/]+)\/(.+)$/,
    transform: (m) => `${ORIGIN}/raw/${m[1]}/${m[2]}/${m[3]}/${m[4]}`,
  },
  {
    label: 'githubAvatar', color: '#fcd34d',
    pattern: /^https?:\/\/github\.com\/([^/]+)\.png$/,
    transform: (m) => `${ORIGIN}/avatar/${m[1]}.png`,
  },
  {
    label: 'npmUnpkg', color: '#f9a8d4',
    pattern: /^https?:\/\/unpkg\.com\/(.+)$/,
    transform: (m) => `${ORIGIN}/unpkg/${m[1]}`,
  },
]

export function useLinkTransform() {
  const input = ref('')
  const copied = ref(false)

  const result = computed<TransformResult | null>(() => {
    const url = input.value.trim().split('?')[0] ?? ''
    if (!url) return null
    for (const rule of rules) {
      const m = url.match(rule.pattern)
      if (m) {
        return { label: rule.label, color: rule.color, url: rule.transform(m), source: url }
      }
    }
    return null
  })

  const hasError = computed(() => input.value.trim().length > 0 && !result.value)

  function clear() { input.value = '' }

  function copyResult() {
    if (!result.value) return
    navigator.clipboard.writeText(result.value.url).then(() => {
      copied.value = true
      setTimeout(() => (copied.value = false), 2200)
    })
  }

  function openResult() {
    if (!result.value) return
    window.open(result.value.url, '_blank', 'noopener,noreferrer')
  }

  function fill(url: string) { input.value = url }

  return { input, result, hasError, copied, clear, copyResult, openResult, fill }
}
