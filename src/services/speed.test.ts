import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => ({
    bytes: 5_000_000,
    elapsedMs: 5000,
    speedBps: 1_000_000,
  })),
}))

import type { InteractableProxyMember } from '@/types/proxy-view'

import speedManager from './speed'

const node = (
  name: string,
  source: { kind: string; proxyName?: string } = {
    kind: 'core',
    proxyName: name,
  },
) =>
  ({
    kind: 'node',
    ref: { kind: 'node', name, recordId: `r:${name}` },
    node: {
      recordId: `r:${name}`,
      name,
      history: [],
      source,
    },
  }) as unknown as InteractableProxyMember

const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

let settles = 0
let unsubscribe: () => void

beforeEach(() => {
  settles = 0
  unsubscribe = speedManager.addGroupListener('g', () => {
    settles += 1
  })
})

afterEach(() => unsubscribe())

describe('状态转换', () => {
  test('未测试 → 测量中 → 有结果', async () => {
    const member = node('a')
    speedManager.setListener('a', 'g', () => {})

    const pending = speedManager.checkSpeed(member, 'g')
    expect(speedManager.getSpeed('a', 'g')).toBe(-2)

    const update = await pending
    expect(update.speed).toBe(1_000_000)
    expect(speedManager.getSpeed('a', 'g')).toBe(1_000_000)
  })

  test('后端命令失败 → 失败状态(0),不滞留测量中', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('SPEEDTEST_FAILED'))

    const update = await speedManager.checkSpeed(node('bad'), 'g')
    expect(update.speed).toBe(0)
    expect(speedManager.getSpeed('bad', 'g')).toBe(0)
  })

  test('前端兜底超时 → 失败状态(0)', async () => {
    vi.useFakeTimers()
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      // 后端一直不返回,触发前端兜底超时(窗口 + 10s)。
      vi.mocked(invoke).mockImplementationOnce(() => new Promise(() => {}))

      const pending = speedManager.checkSpeed(
        node('hang'),
        'g',
        'https://example.com/file',
        1,
      )
      // 500ms 补齐 + 兜底超时。
      await vi.advanceTimersByTimeAsync(1000 + 10_000 + 1000)

      const update = await pending
      expect(update.speed).toBe(0)
      expect(speedManager.getSpeed('hang', 'g')).toBe(0)
    } finally {
      vi.useRealTimers()
    }
  })

  test('provider 节点用 mihomo 侧名称调用命令', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    await speedManager.checkSpeed(
      node('显示名', { kind: 'provider', proxyName: 'api-name' }),
      'g',
    )

    expect(invoke).toHaveBeenCalledWith('speedtest_node', {
      name: 'api-name',
      url: expect.any(String),
      durationSecs: expect.any(Number),
    })
  })
})

describe('TTL 过期', () => {
  test('超过 30 分钟后缓存视为未测试', async () => {
    vi.useFakeTimers()
    try {
      // 假定时器下手动推进 500ms 补齐窗口,让测量落定。
      const pending = speedManager.checkSpeed(node('ttl'), 'g')
      await vi.advanceTimersByTimeAsync(600)
      await pending
      expect(speedManager.getSpeed('ttl', 'g')).toBe(1_000_000)

      vi.setSystemTime(Date.now() + 31 * 60 * 1000)
      expect(speedManager.getSpeed('ttl', 'g')).toBe(-1)
      expect(speedManager.getSpeedUpdate('ttl', 'g')).toBeUndefined()
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('监听通知', () => {
  test('单项监听按序收到测量中与结果', async () => {
    const seen: number[] = []
    speedManager.setListener('n', 'g', (update) => seen.push(update.speed))

    await speedManager.checkSpeed(node('n'), 'g')
    await flush()

    expect(seen).toEqual([-2, 1_000_000])
  })

  test('单次测量完成后通知一次分组监听器', async () => {
    await speedManager.checkSpeed(node('a'), 'g')
    await flush()

    expect(settles).toBe(1)
  })

  test('只通知本组监听器', async () => {
    let other = 0
    const stop = speedManager.addGroupListener('other', () => {
      other += 1
    })

    await speedManager.checkSpeed(node('a'), 'g')
    await flush()

    expect(settles).toBe(1)
    expect(other).toBe(0)
    stop()
  })
})
