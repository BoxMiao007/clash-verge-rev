import { invoke } from '@tauri-apps/api/core'

import type {
  InteractableProxyMember,
  ResolvedProxyMember,
} from '@/types/proxy-view'
import { debugLog } from '@/utils/debug'
import {
  SPEED_TESTING,
  DEFAULT_SPEEDTEST_URL,
  DEFAULT_SPEEDTEST_WINDOW_SECS,
} from '@/utils/speed'

export type SpeedSnapshot = {
  of: (member: ResolvedProxyMember) => number
}

const hashKey = (name: string, group: string) => `${group ?? ''}::${name}`

export interface SpeedUpdate {
  /** 字节/秒;-2 测量中;0 失败;>0 有结果(语义见 utils/speed.ts)。 */
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

class SpeedManager {
  private cache = new Map<string, SpeedUpdate>()

  private listenerMap = new Map<string, (update: SpeedUpdate) => void>()

  private groupListenerMap = new Map<string, Set<() => void>>()
  // Consumers compare snapshot identity; replace it only when the group settles.
  private groupSnapshots = new Map<string, SpeedSnapshot>()

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

  /** 与延迟同一套测量状态机:测量中 → 有结果/失败;结果不足 500ms 补齐,避免状态闪烁。 */
  async checkSpeed(
    member: InteractableProxyMember,
    group: string,
    url: string = DEFAULT_SPEEDTEST_URL,
    windowSecs: number = DEFAULT_SPEEDTEST_WINDOW_SECS,
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
      // 后端负责限时与恢复 GLOBAL;前端超时仅兜底,防止异常时滞留测量中。
      const timeoutPromise = new Promise<SpeedTestResult>((resolve) => {
        setTimeout(
          () => resolve({ bytes: 0, elapsedMs: 0, speedBps: 0 }),
          windowSecs * 1000 + 10_000,
        )
      })

      const result = await Promise.race([
        invoke<SpeedTestResult>('speedtest_node', {
          name: apiName,
          url,
          durationSecs: windowSecs,
        }),
        timeoutPromise,
      ])

      const elapsedTime = Date.now() - startTime
      if (elapsedTime < 500) {
        await new Promise((resolve) => setTimeout(resolve, 500 - elapsedTime))
      }

      const elapsed = Date.now() - startTime
      debugLog(
        `[SpeedManager] 速度测试完成，代理: ${name}, 速度: ${result.speedBps} B/s`,
      )
      const update = this.setSpeed(name, group, result.speedBps, { elapsed })
      this.queueGroupNotification(group)
      return update
    } catch (error) {
      const elapsedTime = Date.now() - startTime
      if (elapsedTime < 500) {
        await new Promise((resolve) => setTimeout(resolve, 500 - elapsedTime))
      }

      console.error(`[SpeedManager] 速度测试出错，代理: ${name}`, error)
      const elapsed = Date.now() - startTime
      // 0 即失败态(下载失败/节点不通/超时一律落失败,不滞留测量中)。
      const update = this.setSpeed(name, group, 0, { elapsed })
      this.queueGroupNotification(group)
      return update
    }
  }
}

export default new SpeedManager()
