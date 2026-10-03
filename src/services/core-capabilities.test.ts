import { describe, expect, test } from 'vitest'

import {
  getCoreDisplayName,
  normalizeCoreCapabilities,
} from './core-capabilities'

describe('normalizeCoreCapabilities', () => {
  test('treats a missing payload as the mihomo baseline so mihomo UI never greys', () => {
    const caps = normalizeCoreCapabilities(undefined)

    expect(caps.core).toBe('verge-mihomo')
    expect(caps.udpConnectionTracking).toBe(true)
    expect(caps.ruleHitCounting).toBe(true)
    expect(caps.listenerHotReload).toBe(true)
    expect(caps.geoUpdateChannel).toBe('kernelApi')
    expect(caps.coreUpgradeChannel).toBe('forkSide')
  })

  test('keeps a meow payload greyed instead of resetting it to the baseline', () => {
    const caps = normalizeCoreCapabilities({
      core: 'verge-meow',
      udpConnectionTracking: false,
      ruleHitCounting: false,
      listenerHotReload: false,
      geoUpdateChannel: 'forkSide',
      coreUpgradeChannel: 'forkSide',
    })

    expect(caps.udpConnectionTracking).toBe(false)
    expect(caps.ruleHitCounting).toBe(false)
    expect(caps.listenerHotReload).toBe(false)
    expect(caps.geoUpdateChannel).toBe('forkSide')
  })
})

describe('getCoreDisplayName', () => {
  test('names the meow core and keeps mihomo cores on mihomo labels', () => {
    expect(getCoreDisplayName('verge-meow')).toBe('Meow')
    expect(getCoreDisplayName('verge-mihomo')).toBe('Mihomo')
    expect(getCoreDisplayName('verge-mihomo-alpha')).toBe('Mihomo')
  })

  test('falls back to the default core label for unknown values', () => {
    expect(getCoreDisplayName(undefined)).toBe('Mihomo')
    expect(getCoreDisplayName('')).toBe('Mihomo')
  })
})

/**
 * 工单 07:能力差异标注文案在所有 locale 都必须存在(zh 中文,其余暂复用英文),
 * 防止新增语言或改名后标注静默退化为裸键名。
 */
describe('capability notice copy', () => {
  type LocaleBundle = { page?: { notices?: Record<string, string> } }

  // 与 services/i18n.ts 同样的 glob 解析:路径取 <语言>/<命名空间>
  const localeModules = import.meta.glob<{ default: LocaleBundle }>(
    '@/locales/*/*.json',
    { eager: true },
  )
  const localeNames = new Set<string>()
  const notices = new Map<string, Record<string, string | undefined>>()

  for (const [path, module] of Object.entries(localeModules)) {
    const match = path.match(/[/\\]locales[/\\]([^/\\]+)[/\\]([^/\\]+)\.json$/)
    if (!match) continue
    const [, locale, namespace] = match
    localeNames.add(locale)
    notices.set(
      `${locale}/${namespace}`,
      module.default.page?.notices ?? {},
    )
  }

  const readNotice = (locale: string, namespace: string, key: string) =>
    notices.get(`${locale}/${namespace}`)?.[key]

  test('every locale carries both capability notices', () => {
    // 至少覆盖 zh 与 en,新增语言自动纳入校验
    expect(localeNames.has('zh')).toBe(true)
    expect(localeNames.has('en')).toBe(true)
    for (const locale of localeNames) {
      expect(
        readNotice(locale, 'connections', 'udpUnavailable'),
        `${locale}/connections 缺 udpUnavailable`,
      ).toBeTruthy()
      expect(
        readNotice(locale, 'rules', 'hitCountUnavailable'),
        `${locale}/rules 缺 hitCountUnavailable`,
      ).toBeTruthy()
    }
  })

  test('notices interpolate the core label and explain the capability gap', () => {
    for (const [namespace, key] of [
      ['connections', 'udpUnavailable'],
      ['rules', 'hitCountUnavailable'],
    ] as const) {
      for (const locale of localeNames) {
        const text = readNotice(locale, namespace, key) ?? ''
        expect(text).toContain('{{core}}')
        // zh 用中文文案,其余 locale 暂复用英文
        expect(text).toContain(locale === 'zh' ? '内核' : 'kernel')
      }
    }
  })
})
