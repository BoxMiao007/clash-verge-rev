import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => ({
    bytes: 5_000_000,
    elapsedMs: 5000,
    speedBps: 1_000_000,
  })),
}))

import type { InteractableProxyMember } from '@/types/proxy-view'
import { SPEED_TIMEOUT } from '@/utils/speed'

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

  test('前端兜底超时 → 超时状态(-3),与后端错误的失败态区分', async () => {
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
      expect(update.speed).toBe(SPEED_TIMEOUT)
      expect(speedManager.getSpeed('hang', 'g')).toBe(SPEED_TIMEOUT)
    } finally {
      vi.useRealTimers()
    }
  })

  test('后端命令失败与兜底超时可区分:失败 0,超时 -3', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('SPEEDTEST_FAILED'))

    const failed = await speedManager.checkSpeed(node('bad'), 'g')
    expect(failed.speed).toBe(0)
    expect(failed.speed).not.toBe(SPEED_TIMEOUT)
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

describe('流量上限透传', () => {
  test('单项测速把 maxBytes 原样传给后端命令', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    await speedManager.checkSpeed(
      node('cap'),
      'g',
      'https://example.com/file',
      1,
      1_048_576,
    )

    expect(invoke).toHaveBeenCalledWith('speedtest_node', {
      name: 'cap',
      url: 'https://example.com/file',
      durationSecs: 1,
      maxBytes: 1_048_576,
    })
  })

  test('未传流量上限时 maxBytes 为 undefined(后端不限,既有调用向后兼容)', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    await speedManager.checkSpeed(node('nocap'), 'g')

    expect(invoke).toHaveBeenCalledWith('speedtest_node', {
      name: 'nocap',
      url: expect.any(String),
      durationSecs: expect.any(Number),
      maxBytes: undefined,
    })
  })

  test('整组测速把 maxBytes 原样传给后端命令', async () => {
    vi.useFakeTimers()
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const pending = speedManager.checkListSpeed(
        [node('c1')],
        'cap-batch',
        'https://example.com/file',
        1,
        1_048_576,
      )
      await vi.advanceTimersByTimeAsync(2000)
      await pending

      expect(invoke).toHaveBeenCalledWith('speedtest_node', {
        name: 'c1',
        url: 'https://example.com/file',
        durationSecs: 1,
        maxBytes: 1_048_576,
      })
    } finally {
      vi.useRealTimers()
    }
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

type Gate = {
  promise: Promise<{ bytes: number; elapsedMs: number; speedBps: number }>
  release: (value: {
    bytes: number
    elapsedMs: number
    speedBps: number
  }) => void
}

const manualGate = (): Gate => {
  let release!: Gate['release']
  const promise = new Promise<
    Gate['promise'] extends Promise<infer V> ? V : never
  >((resolve) => {
    release = resolve
  })
  return { promise, release }
}

describe('批量串行调度', () => {
  test('严格串行:前一节点完成才发起下一个,单项结果逐个通知,分组通知延后到收尾', async () => {
    vi.useFakeTimers()
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const members = [node('b1'), node('b2'), node('b3')]

      const gates = members.map(() => manualGate())
      const calls: string[] = []
      members.forEach((member, index) => {
        vi.mocked(invoke).mockImplementationOnce((_cmd, args) => {
          calls.push((args as { name: string }).name)
          return gates[index].promise
        })
      })

      const seen: Array<[string, number]> = []
      members.forEach(({ ref }) =>
        speedManager.setListener(ref.name, 'batch', (update) =>
          seen.push([ref.name, update.speed]),
        ),
      )

      let settles = 0
      const unsubscribe = speedManager.addGroupListener('batch', () => {
        settles += 1
      })

      try {
        const pending = speedManager.checkListSpeed(
          members,
          'batch',
          'https://example.com/file',
          1,
        )
        await vi.advanceTimersByTimeAsync(0)
        // 整组先进入测量中,但只发起了第一个节点 → 严格串行。
        expect(calls).toEqual(['b1'])
        expect(speedManager.getSpeed('b2', 'batch')).toBe(-2)

        gates[0].release({ bytes: 1, elapsedMs: 1, speedBps: 111 })
        await vi.advanceTimersByTimeAsync(600)
        // b1 完成即更新单项显示并发起 b2;分组通知被批量抑制。
        expect(seen).toContainEqual(['b1', 111])
        expect(calls).toEqual(['b1', 'b2'])
        expect(settles).toBe(0)

        gates[1].release({ bytes: 1, elapsedMs: 1, speedBps: 222 })
        await vi.advanceTimersByTimeAsync(600)
        expect(seen).toContainEqual(['b2', 222])
        expect(calls).toEqual(['b1', 'b2', 'b3'])
        expect(settles).toBe(0)

        gates[2].release({ bytes: 1, elapsedMs: 1, speedBps: 333 })
        // 先推进末位的 500ms 补齐窗口,再等整批收尾。
        await vi.advanceTimersByTimeAsync(600)
        await pending
        await vi.advanceTimersByTimeAsync(0)
        expect(seen).toContainEqual(['b3', 333])
        // 收尾才通知一次分组监听器。
        expect(settles).toBe(1)
      } finally {
        members.forEach(({ ref }) =>
          speedManager.removeListener(ref.name, 'batch'),
        )
        unsubscribe()
      }
    } finally {
      vi.useRealTimers()
    }
  })

  test('失败节点落失败态(0),不阻塞其余节点继续', async () => {
    vi.useFakeTimers()
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      vi.mocked(invoke)
        .mockImplementationOnce(async () => ({
          bytes: 1,
          elapsedMs: 1,
          speedBps: 111,
        }))
        .mockRejectedValueOnce(new Error('SPEEDTEST_FAILED'))
        .mockImplementationOnce(async () => ({
          bytes: 1,
          elapsedMs: 1,
          speedBps: 333,
        }))

      const pending = speedManager.checkListSpeed(
        [node('f1'), node('f2'), node('f3')],
        'fail-batch',
        'https://example.com/file',
        1,
      )
      await vi.advanceTimersByTimeAsync(2000)
      await pending

      expect(speedManager.getSpeed('f1', 'fail-batch')).toBe(111)
      expect(speedManager.getSpeed('f2', 'fail-batch')).toBe(0)
      expect(speedManager.getSpeed('f3', 'fail-batch')).toBe(333)
    } finally {
      vi.useRealTimers()
    }
  })

  test('进行中重复触发:分组通知在所有批次收尾后仅一次', async () => {
    vi.useFakeTimers()
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const firstGate = manualGate()
      const secondGate = manualGate()
      vi.mocked(invoke)
        .mockImplementationOnce(() => firstGate.promise)
        .mockImplementationOnce(() => secondGate.promise)

      let settles = 0
      const unsubscribe = speedManager.addGroupListener('overlap', () => {
        settles += 1
      })

      try {
        const first = speedManager.checkListSpeed(
          [node('o1')],
          'overlap',
          'https://example.com/file',
          1,
        )
        const second = speedManager.checkListSpeed(
          [node('o2')],
          'overlap',
          'https://example.com/file',
          1,
        )
        await vi.advanceTimersByTimeAsync(0)
        expect(settles).toBe(0)

        firstGate.release({ bytes: 1, elapsedMs: 1, speedBps: 1 })
        await vi.advanceTimersByTimeAsync(600)
        await first
        await vi.advanceTimersByTimeAsync(0)
        // 第二批仍在进行,抑制保持。
        expect(settles).toBe(0)

        secondGate.release({ bytes: 1, elapsedMs: 1, speedBps: 2 })
        await vi.advanceTimersByTimeAsync(600)
        await second
        await vi.advanceTimersByTimeAsync(0)
        expect(settles).toBe(1)
      } finally {
        unsubscribe()
      }
    } finally {
      vi.useRealTimers()
    }
  })
})
