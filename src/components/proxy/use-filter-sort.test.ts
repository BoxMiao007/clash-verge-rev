import { afterEach, expect, test, vi } from 'vitest'

import delayManager from '@/services/delay'
import speedManager from '@/services/speed'
import { compareByDelay } from '@/utils/delay'
import { SPEED_TESTING } from '@/utils/speed'

import { filterSort } from './use-filter-sort'
import type { ResolvedMemberOccurrence } from './use-render-list'

const node = (memberIndex: number, delay: number, name = `${memberIndex}`) =>
  ({
    memberIndex,
    member: {
      kind: 'node',
      ref: { kind: 'node', name, recordId: `${memberIndex}` },
      node: {
        history: [{ delay }],
        source: { kind: 'provider', providerName: `${memberIndex}` },
      },
    },
  }) as ResolvedMemberOccurrence

afterEach(() => vi.restoreAllMocks())

test('matches the previous comparator for cached, fallback and sentinel delays', () => {
  vi.spyOn(Date, 'now').mockReturnValue(1000)
  const values = [30, 0, -2, -1, 1e6, 10000, 30, NaN, Infinity]
  const list = values.map((delay, i) => node(i, delay))
  list.push(node(10, 50, 'same'), node(11, 5, 'same'), list[0])
  list.push({
    memberIndex: 12,
    member: {
      kind: 'unresolved',
      ref: { kind: 'unresolved', name: 'missing', reason: 'missing' },
    },
  })
  for (const cached of [false, true]) {
    const group = `sort-${cached}`
    if (cached) {
      values.forEach((delay, i) => delayManager.setDelay(`${i}`, group, delay))
    }
    for (const timeout of [10000, 20, 0, NaN]) {
      const expected = list
        .slice()
        .sort((a, b) =>
          compareByDelay(
            delayManager.getDelayFix(a.member, group),
            delayManager.getDelayFix(b.member, group),
            timeout > 0 ? timeout : 10000,
          ),
        )
      const before = list.slice()
      const result = filterSort(list, group, '', 1, timeout)
      expect(result).toEqual(expected)
      result.forEach((item, i) => expect(item).toBe(expected[i]))
      expect(list).toEqual(before)
    }
  }
})

test('reads each occurrence once and observes cache updates and expiry on the next call', () => {
  const clock = vi.spyOn(Date, 'now').mockReturnValue(1000)
  const list = [
    node(0, 50),
    node(1, 15, 'same'),
    node(2, 5, 'same'),
    node(3, 25),
  ]
  const cachedOrder = [list[0], list[2], list[1], list[3]]
  delayManager.setDelay('0', 'expiry', 1)
  const get = vi.spyOn(delayManager, 'getDelayFix')
  expect(filterSort(list, 'expiry', '', 1)).toEqual(cachedOrder)
  expect(get).toHaveBeenCalledTimes(list.length)
  list.forEach((item, i) => expect(get.mock.calls[i][0]).toBe(item.member))
  clock.mockReturnValue(1000 + 30 * 60 * 1000)
  expect(filterSort(list, 'expiry', '', 1)).toEqual(cachedOrder)
  clock.mockReturnValue(1001 + 30 * 60 * 1000)
  expect(filterSort(list, 'expiry', '', 1)).toEqual([
    list[2],
    list[1],
    list[3],
    list[0],
  ])
  delayManager.setDelay('0', 'expiry', 2)
  expect(filterSort(list, 'expiry', '', 1)).toEqual(cachedOrder)
  get.mockClear()
  filterSort([], 'expiry', '', 1)
  filterSort([list[0]], 'expiry', '', 1)
  expect(filterSort(list, 'expiry', '', 0)).toBe(list)
  filterSort(list, 'expiry', '', 2)
  expect(get).not.toHaveBeenCalled()
})

test('speed tier sorts measured descending first and the rest last, keeping input order on ties', () => {
  const group = 'sort-speed'
  const list = [node(0, 0), node(1, 0), node(2, 0), node(3, 0), node(4, 0)]
  // 名称即 `${memberIndex}`:3 有结果(最快)、2 有结果(较慢)、1 测量中、4 失败、0 未测试。
  speedManager.setSpeed('2', group, 500_000)
  speedManager.setSpeed('3', group, 2_000_000)
  speedManager.setSpeed('1', group, SPEED_TESTING)
  speedManager.setSpeed('4', group, 0)

  const result = filterSort(list, group, '', 3)

  const expectedNames = ['3', '2', '1', '4', '0']
  expect(result.map(({ member }) => member.ref.name)).toEqual(expectedNames)
  // 排序不得复制成员对象:结果里仍是原列表中的同一 occurrence。
  expectedNames.forEach((name, i) => expect(result[i]).toBe(list[Number(name)]))
})
