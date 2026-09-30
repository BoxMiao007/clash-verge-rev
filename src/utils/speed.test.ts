import { describe, expect, test } from 'vitest'

import {
  DEFAULT_SPEEDTEST_WINDOW_SECS,
  DEFAULT_SPEEDTEST_URL,
  MAX_SPEEDTEST_DURATION_SECS,
  MIN_SPEEDTEST_DURATION_SECS,
  SPEED_TESTING,
  SPEED_TIMEOUT,
  clampSpeedtestDurationSecs,
  compareBySpeed,
  classifySpeed,
  formatSpeed,
  formatSpeedColor,
  resolveSpeedtestDurationSecs,
  resolveSpeedtestUrl,
} from './speed'

describe('classifySpeed', () => {
  test('语义归一与延迟状态机一致', () => {
    expect(classifySpeed(SPEED_TESTING)).toBe('testing')
    expect(classifySpeed(SPEED_TIMEOUT)).toBe('timeout')
    expect(classifySpeed(-1)).toBe('untested')
    expect(classifySpeed(Number.NaN)).toBe('untested')
    expect(classifySpeed(0)).toBe('failed')
    expect(classifySpeed(1024)).toBe('measured')
  })
})

describe('formatSpeed', () => {
  test('KB/s 取整数', () => {
    expect(formatSpeed(100 * 1024)).toBe('100 KB/s')
    expect(formatSpeed(100 * 1024 + 512)).toBe('101 KB/s')
  })

  test('MB/s 一位小数', () => {
    expect(formatSpeed(1024 * 1024)).toBe('1.0 MB/s')
    expect(formatSpeed(2.56 * 1024 * 1024)).toBe('2.6 MB/s')
  })

  test('状态文本', () => {
    expect(formatSpeed(SPEED_TESTING)).toBe('testing')
    expect(formatSpeed(SPEED_TIMEOUT)).toBe('Timeout')
    expect(formatSpeed(-1)).toBe('-')
    expect(formatSpeed(0)).toBe('Failed')
  })
})

describe('formatSpeedColor', () => {
  test('超时/失败红色,结果按量级分色', () => {
    expect(formatSpeedColor(0)).toBe('error.main')
    expect(formatSpeedColor(SPEED_TIMEOUT)).toBe('error.main')
    expect(formatSpeedColor(100 * 1024)).toBe('warning.main')
    expect(formatSpeedColor(2 * 1024 * 1024)).toBe('primary.main')
    expect(formatSpeedColor(8 * 1024 * 1024)).toBe('success.main')
    expect(formatSpeedColor(-1)).toBe('')
    expect(formatSpeedColor(SPEED_TESTING)).toBe('')
  })
})

describe('compareBySpeed', () => {
  test('有结果降序,越大越前', () => {
    expect(compareBySpeed(3 * 1024 * 1024, 1024 * 1024)).toBeLessThan(0)
    expect(compareBySpeed(1024 * 1024, 3 * 1024 * 1024)).toBeGreaterThan(0)
    expect(compareBySpeed(1024 * 1024, 1024 * 1024)).toBe(0)
  })

  test('非测量结果按 超时 > 失败 > 测量中 > 未测试 排后,彼此不分先后', () => {
    expect(compareBySpeed(1024, 0)).toBeLessThan(0)
    expect(compareBySpeed(1024, SPEED_TIMEOUT)).toBeLessThan(0)
    expect(compareBySpeed(SPEED_TIMEOUT, 0)).toBeLessThan(0)
    expect(compareBySpeed(0, -1)).toBeLessThan(0)
    expect(compareBySpeed(-1, SPEED_TESTING)).toBeGreaterThan(0)
    // 测量中排在失败之后(与延迟 rank 同序)。
    expect(compareBySpeed(SPEED_TESTING, 0)).toBeGreaterThan(0)
    expect(compareBySpeed(0, 0)).toBe(0)
    expect(compareBySpeed(SPEED_TIMEOUT, SPEED_TIMEOUT)).toBe(0)
  })
})

describe('clampSpeedtestDurationSecs', () => {
  test('区间内原样返回,越界夹到边界', () => {
    expect(clampSpeedtestDurationSecs(5)).toBe(5)
    expect(clampSpeedtestDurationSecs(0)).toBe(MIN_SPEEDTEST_DURATION_SECS)
    expect(clampSpeedtestDurationSecs(120)).toBe(MAX_SPEEDTEST_DURATION_SECS)
  })
})

describe('默认参数', () => {
  test('默认测速 URL 与测速窗口', () => {
    expect(DEFAULT_SPEEDTEST_URL).toContain('googlechrome.dmg')
  })
})

describe('resolveSpeedtestUrl', () => {
  test('已配置返回去除首尾空白后的值', () => {
    expect(resolveSpeedtestUrl(' https://example.com/file ')).toBe(
      'https://example.com/file',
    )
  })

  test('空白或未配置回落内置默认', () => {
    expect(resolveSpeedtestUrl('')).toBe(DEFAULT_SPEEDTEST_URL)
    expect(resolveSpeedtestUrl('   ')).toBe(DEFAULT_SPEEDTEST_URL)
    expect(resolveSpeedtestUrl(undefined)).toBe(DEFAULT_SPEEDTEST_URL)
    expect(resolveSpeedtestUrl(null)).toBe(DEFAULT_SPEEDTEST_URL)
  })
})

describe('resolveSpeedtestDurationSecs', () => {
  test('区间内的整数原样返回', () => {
    expect(resolveSpeedtestDurationSecs(1)).toBe(1)
    expect(resolveSpeedtestDurationSecs(5)).toBe(5)
    expect(resolveSpeedtestDurationSecs(MAX_SPEEDTEST_DURATION_SECS)).toBe(
      MAX_SPEEDTEST_DURATION_SECS,
    )
  })

  test('超范围夹到区间边界', () => {
    expect(resolveSpeedtestDurationSecs(0)).toBe(MIN_SPEEDTEST_DURATION_SECS)
    expect(resolveSpeedtestDurationSecs(-3)).toBe(MIN_SPEEDTEST_DURATION_SECS)
    expect(resolveSpeedtestDurationSecs(120)).toBe(MAX_SPEEDTEST_DURATION_SECS)
  })

  test('非法值回落内置默认', () => {
    expect(resolveSpeedtestDurationSecs(Number.NaN)).toBe(
      DEFAULT_SPEEDTEST_WINDOW_SECS,
    )
    expect(resolveSpeedtestDurationSecs(2.5)).toBe(
      DEFAULT_SPEEDTEST_WINDOW_SECS,
    )
    expect(resolveSpeedtestDurationSecs(undefined)).toBe(
      DEFAULT_SPEEDTEST_WINDOW_SECS,
    )
    expect(resolveSpeedtestDurationSecs(null)).toBe(
      DEFAULT_SPEEDTEST_WINDOW_SECS,
    )
  })
})
