import { invoke } from '@tauri-apps/api/core'

import type {
  InteractableProxyMember,
  ResolvedProxyMember,
} from '@/types/proxy-view'
import { debugLog } from '@/utils/debug'
import {
  SPEED_TESTING,
  SPEED_TIMEOUT,
  DEFAULT_SPEEDTEST_URL,
  DEFAULT_SPEEDTEST_WINDOW_SECS,
} from '@/utils/speed'

type SpeedSnapshot = {
  of: (member: ResolvedProxyMember) => number
}

const hashKey = (name: string, group: string) => `${group ?? ''}::${name}`

export interface SpeedUpdate {
  /** 字节/秒;-2 测量中;-3 超时;0 失败;>0 有结果(语义见 utils/speed.ts)。 */
  speed: number
  elapsed?: number
  updatedAt: number
}

/** 后端命令 speedtest_node 的返回值。 */
interface SpeedTestResult {
  bytes: number
  elapsedMs: number
  speedBps: number
}

const CACHE_TTL = 30 * 60 * 1000

/** 结果落定前的展示补齐下限:整体不足该时长时等待补足,避免状态闪烁过快。 */
const MIN_MEASURE_DISPLAY_MS = 500

/** 等待补齐展示时长,返回含补齐的总耗时(毫秒)。
 *  本文件内的重复块提取;src/services/delay.ts 存在同样的内联写法,属上游文件不动。 */
async function padToMinDisplayMs(startTime: number): Promise<number> {
  const elapsedBefore = Date.now() - startTime
  if (elapsedBefore < MIN_MEASURE_DISPLAY_MS) {
    await new Promise((resolve) =>
      setTimeout(resolve, MIN_MEASURE_DISPLAY_MS - elapsedBefore),
    )
  }
  return Date.now() - startTime
}

class SpeedManager {
  private cache = new Map<string, SpeedUpdate>()

  private listenerMap = new Map<string, (update: SpeedUpdate) => void>()

  private groupListenerMap = new Map<string, Set<() => void>>()
  // Consumers compare snapshot identity; replace it only when the group settles.
  private groupSnapshots = new Map<string, SpeedSnapshot>()

  // Suppress sort notifications until every measurement in a group batch settles.
  private activeBatches = new Map<string, number>()

  private pendingItemUpdates = new Map<string, SpeedUpdate[]>()
  private pendingGroupUpdates = new Set<string>()
  private itemFlushScheduled = false
  private groupFlushScheduled = false

  private scheduleOnNextFrame(run: () => void): void {
    if (typeof window !== 'undefined') {
      if (typeof window.requestAnimationFrame === 'function') {
        window.requestAnimationFrame(run)
        return
      }
      if (typeof window.setTimeout === 'function') {
        window.setTimeout(run, 0)
        return
      }
    }

    Promise.resolve().then(run)
  }

  private scheduleItemFlush() {
    if (this.itemFlushScheduled) return
    this.itemFlushScheduled = true

    this.scheduleOnNextFrame(() => {
      this.itemFlushScheduled = false
      const updates = this.pendingItemUpdates
      this.pendingItemUpdates = new Map()

      updates.forEach((queue, key) => {
        const listener = this.listenerMap.get(key)
        if (!listener) return

        queue.forEach((update) => {
          try {
            listener(update)
          } catch (error) {
            console.error(
              `[SpeedManager] 通知节点速度监听器失败: ${key}`,
              error,
            )
          }
        })
      })
    })
  }

  private scheduleGroupFlush() {
    if (this.groupFlushScheduled) return
    this.groupFlushScheduled = true

    this.scheduleOnNextFrame(() => {
      this.groupFlushScheduled = false
      const groups = this.pendingGroupUpdates
      this.pendingGroupUpdates = new Set()

      groups.forEach((group) => {
        const listeners = this.groupListenerMap.get(group)
        if (!listeners) return
        // Copied before iterating: a listener is free to unsubscribe as it runs.
        for (const listener of [...listeners]) {
          try {
            listener()
          } catch (error) {
            console.error(
              `[SpeedManager] 通知分组速度监听器失败: ${group}`,
              error,
            )
          }
        }
      })
    })
  }

  private queueGroupNotification(group: string) {
    if ((this.activeBatches.get(group) ?? 0) > 0) return
    this.groupSnapshots.delete(group)
    this.pendingGroupUpdates.add(group)
    this.scheduleGroupFlush()
  }

  /** Cached group snapshot whose identity changes only when that group settles. */
  groupSpeeds(group: string): SpeedSnapshot {
    const existing = this.groupSnapshots.get(group)
    if (existing) return existing

    const snapshot: SpeedSnapshot = {
      of: (member) => this.getSpeed(member.ref.name, group),
    }
    this.groupSnapshots.set(group, snapshot)
    return snapshot
  }

  setListener(
    name: string,
    group: string,
    listener: (update: SpeedUpdate) => void,
  ) {
    const key = hashKey(name, group)
    this.listenerMap.set(key, listener)
  }

  removeListener(name: string, group: string) {
    const key = hashKey(name, group)
    this.listenerMap.delete(key)
  }

  /** Multiple views may subscribe independently; notifications occur only after settle. */
  addGroupListener(group: string, listener: () => void): () => void {
    const listeners = this.groupListenerMap.get(group) ?? new Set()
    listeners.add(listener)
    this.groupListenerMap.set(group, listeners)

    return () => {
      const current = this.groupListenerMap.get(group)
      if (!current) return
      current.delete(listener)
      if (current.size === 0) this.groupListenerMap.delete(group)
    }
  }

  setSpeed(
    name: string,
    group: string,
    speed: number,
    meta?: { elapsed?: number },
  ): SpeedUpdate {
    const key = hashKey(name, group)
    debugLog(
      `[SpeedManager] 设置速度，代理: ${name}, 组: ${group}, 速度: ${speed}`,
    )
    const update: SpeedUpdate = {
      speed,
      elapsed: meta?.elapsed,
      updatedAt: Date.now(),
    }

    this.cache.set(key, update)

    const queue = this.pendingItemUpdates.get(key)
    if (queue) {
      queue.push(update)
    } else {
      this.pendingItemUpdates.set(key, [update])
    }
    this.scheduleItemFlush()

    return update
  }

  getSpeedUpdate(name: string, group: string) {
    const key = hashKey(name, group)
    const entry = this.cache.get(key)
    if (!entry) return undefined

    if (Date.now() - entry.updatedAt > CACHE_TTL) {
      this.cache.delete(key)
      return undefined
    }

    return { ...entry }
  }

  getSpeed(name: string, group: string) {
    const update = this.getSpeedUpdate(name, group)
    return update ? update.speed : -1
  }

  /** A single test may notify immediately; a batch defers notification until it settles. */
  async checkSpeed(
    member: InteractableProxyMember,
    group: string,
    url: string = DEFAULT_SPEEDTEST_URL,
    windowSecs: number = DEFAULT_SPEEDTEST_WINDOW_SECS,
    maxBytes?: number,
  ): Promise<SpeedUpdate> {
    const update = await this.measureSpeed(member, group, url, windowSecs, maxBytes)
    this.queueGroupNotification(group)
    return update
  }

  private async measureSpeed(
    member: InteractableProxyMember,
    group: string,
    url: string,
    windowSecs: number,
    maxBytes?: number,
  ): Promise<SpeedUpdate> {
    const name = member.ref.name
    const apiName =
      member.kind === 'node' && member.node.source.kind === 'provider'
        ? member.node.source.proxyName
        : name
    debugLog(
      `[SpeedManager] 开始速度测试，代理: ${name}, URL: ${url}, 测速窗口: ${windowSecs}s`,
    )

    this.setSpeed(name, group, SPEED_TESTING)

    const startTime = Date.now()

    try {
      // 后端负责限时与恢复 GLOBAL;前端兜底超时仅防异常时滞留测量中,归入超时态。
      const fallback = new Promise<number>((resolve) => {
        setTimeout(() => resolve(SPEED_TIMEOUT), windowSecs * 1000 + 10_000)
      })

      // maxBytes 为 undefined 时 JSON 序列化会丢弃该键,后端 Option 收到 None 即不限。
      const result = await Promise.race([
        invoke<SpeedTestResult>('speedtest_node', {
          name: apiName,
          url,
          durationSecs: windowSecs,
          maxBytes,
        }),
        fallback,
      ])

      if (typeof result === 'number') {
        // 兜底超时触发:命令迟迟未返回,与后端下载错误的失败态区分。
        debugLog(`[SpeedManager] 前端兜底超时，代理: ${name}`)
        const elapsed = await padToMinDisplayMs(startTime)
        return this.setSpeed(name, group, result, { elapsed })
      }

      const elapsed = await padToMinDisplayMs(startTime)
      debugLog(
        `[SpeedManager] 速度测试完成，代理: ${name}, 速度: ${result.speedBps} B/s`,
      )
      return this.setSpeed(name, group, result.speedBps, { elapsed })
    } catch (error) {
      const elapsed = await padToMinDisplayMs(startTime)
      console.error(`[SpeedManager] 速度测试出错，代理: ${name}`, error)
      // 0 即失败态:后端下载错误/HTTP 非 2xx/窗口内 0 字节,不滞留测量中。
      return this.setSpeed(name, group, 0, { elapsed })
    }
  }

  /** 整组测速:严格串行(切换 GLOBAL 互斥,并发是错误形态),失败节点不阻塞其余;单项通知即时、分组通知延后到收尾。 */
  async checkListSpeed(
    proxies: InteractableProxyMember[],
    group: string,
    url: string = DEFAULT_SPEEDTEST_URL,
    windowSecs: number = DEFAULT_SPEEDTEST_WINDOW_SECS,
    maxBytes?: number,
  ) {
    debugLog(
      `[SpeedManager] 批量测试速度开始，组: ${group}, 数量: ${proxies.length}`,
    )
    this.activeBatches.set(group, (this.activeBatches.get(group) ?? 0) + 1)
    proxies.forEach(({ ref }) => {
      this.setSpeed(ref.name, group, SPEED_TESTING)
    })

    const startTime = Date.now()

    try {
      for (const member of proxies) {
        const name = member.ref.name
        try {
          await this.measureSpeed(member, group, url, windowSecs, maxBytes)
        } catch (error) {
          // 单个节点的意外异常不拖垮整组;结果落失败态,继续下一个。
          console.error(
            `[SpeedManager] 批量测试单个代理出错，代理: ${name}`,
            error,
          )
          this.setSpeed(name, group, 0)
        }
      }
    } finally {
      // Always release the batch and notify; otherwise failures leave stale sort state.
      const remaining = (this.activeBatches.get(group) ?? 1) - 1
      if (remaining > 0) {
        this.activeBatches.set(group, remaining)
      } else {
        this.activeBatches.delete(group)
        this.queueGroupNotification(group)
      }
    }
    const totalTime = Date.now() - startTime
    debugLog(
      `[SpeedManager] 批量测试速度完成，组: ${group}, 总耗时: ${totalTime}ms`,
    )
  }
}

export default new SpeedManager()
