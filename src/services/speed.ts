import type {
  InteractableProxyMember,
  ProxyNodeView,
  ResolvedProxyMember,
} from '@/types/proxy-view'
import { debugLog } from '@/utils/debug'
import {
  SPEED_TESTING,
  SPEED_TIMEOUT,
  DEFAULT_SPEEDTEST_URL,
  DEFAULT_SPEEDTEST_WINDOW_SECS,
} from '@/utils/speed'

import { speedtestNode, speedtestNodeViaSlot, speedtestSlotPorts } from './cmds'

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

/** 持久化用的节点标识:节点名 + 来源(provider 名 / core)。
 *  刻意不含分组:同一节点在多个分组共享一条结果,符合“按节点”的语义。 */
export const speedIdentityOfNode = (node: ProxyNodeView): string => {
  const source = node.source
  return source.kind === 'provider'
    ? `p|${source.providerName}|${source.proxyName}`
    : `c|${source.proxyName}`
}

export const speedIdentityOfMember = (
  member: InteractableProxyMember | ResolvedProxyMember,
): string => {
  if (member.kind === 'group') return `g|${member.ref.name}`
  if (member.kind === 'node') return speedIdentityOfNode(member.node)
  return `u|${member.ref.name}`
}

const STORAGE_VERSION = 'v1'
const speedStorageKey = (profileUid: string) =>
  `verge.speedtest.results:${STORAGE_VERSION}:${profileUid}`

type PersistedSpeedResults = Record<string, SpeedUpdate>

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

  /** 当前订阅 uid;持久化按订阅隔离。null 表示尚未确定(不落盘)。 */
  private profileUid: string | null = null

  /** 当前订阅的持久化结果(按节点标识);与会话内 cache 分离,页面刷新后仍可还塡。 */
  private persisted = new Map<string, SpeedUpdate>()

  /** 持久化层变化的全局订阅:跨分组共享的展示据此重新读取。 */
  private storeListeners = new Set<() => void>()

  /** 并行测速槽端口(下标即槽号,null=该槽不可用);惰性读取,失败回落 GLOBAL 通道。 */
  private slotPorts: (number | null)[] | null = null
  private slotLoad: Promise<void> | null = null
  private busySlots = new Set<number>()
  private slotWaiters: Array<(slot: number) => void> = []

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
      of: (member) => this.getSpeedForMember(member, group),
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

  // === 持久化与订阅作用域 =================================================

  /** 切换当前订阅作用域:清空会话缓存并载入该订阅的持久化结果。 */
  setProfileScope(profileUid: string | null) {
    if (this.profileUid === profileUid) return
    this.profileUid = profileUid
    this.cache.clear()
    this.persisted.clear()
    if (profileUid) this.loadPersisted(profileUid)
    this.notifyStore()
  }

  private loadPersisted(profileUid: string) {
    if (typeof window === 'undefined') return
    try {
      const raw = window.localStorage.getItem(speedStorageKey(profileUid))
      if (!raw) return
      const parsed = JSON.parse(raw) as PersistedSpeedResults
      for (const [key, value] of Object.entries(parsed)) {
        if (value && typeof value.speed === 'number') {
          this.persisted.set(key, value)
        }
      }
    } catch (error) {
      console.error('[SpeedManager] 读取测速结果持久化失败', error)
    }
  }

  private savePersisted() {
    if (!this.profileUid || typeof window === 'undefined') return
    try {
      const plain: PersistedSpeedResults = {}
      this.persisted.forEach((value, key) => {
        plain[key] = value
      })
      window.localStorage.setItem(
        speedStorageKey(this.profileUid),
        JSON.stringify(plain),
      )
    } catch (error) {
      console.error('[SpeedManager] 写入测速结果持久化失败', error)
    }
  }

  /** 手动测速落定后持久化(只保留终态;测量中不落盘)。 */
  private persistResult(
    member: InteractableProxyMember | ResolvedProxyMember,
    update: SpeedUpdate,
  ) {
    if (!this.profileUid || update.speed === SPEED_TESTING) return
    this.persisted.set(speedIdentityOfMember(member), {
      speed: update.speed,
      elapsed: update.elapsed,
      updatedAt: update.updatedAt,
    })
    this.savePersisted()
    this.notifyStore()
  }

  /** 订阅刷新后清理该订阅内已不存在的节点结果;present 为空表示加载不完整,跳过。 */
  pruneProfile(profileUid: string, presentIdentities: Set<string>) {
    if (!this.profileUid || this.profileUid !== profileUid) return
    if (presentIdentities.size === 0) return
    let changed = false
    for (const key of [...this.persisted.keys()]) {
      if (!presentIdentities.has(key)) {
        this.persisted.delete(key)
        changed = true
      }
    }
    if (changed) {
      this.savePersisted()
      this.notifyStore()
    }
  }

  /** 持久化层变化订阅(跨分组共享的展示据此刷新)。 */
  addStoreListener(listener: () => void): () => void {
    this.storeListeners.add(listener)
    return () => {
      this.storeListeners.delete(listener)
    }
  }

  private notifyStore() {
    for (const listener of [...this.storeListeners]) {
      try {
        listener()
      } catch (error) {
        console.error('[SpeedManager] 通知持久化订阅者失败', error)
      }
    }
  }

  // === 并行测速槽池 ======================================================

  private async loadSlots(force = false): Promise<void> {
    if (force) this.slotPorts = null
    if (this.slotPorts) return
    if (this.slotLoad) {
      await this.slotLoad
      return
    }
    this.slotLoad = (async () => {
      try {
        const ports = await speedtestSlotPorts()
        const usable = ports.map((port) =>
          typeof port === 'number' && port > 0 ? port : null,
        )
        this.slotPorts = usable.some((port) => port != null) ? usable : null
      } catch (error) {
        console.error('[SpeedManager] 读取并行测速槽失败,回退 GLOBAL 通道', error)
        this.slotPorts = null
      } finally {
        this.slotLoad = null
      }
    })()
    await this.slotLoad
  }

  private slotAvailable(): number {
    if (!this.slotPorts) return 0
    return this.slotPorts.filter((port) => port != null).length
  }

  /** 借一个空闲槽;无可用槽返回 null(调用方回落 GLOBAL 通道)。 */
  private async acquireSlot(): Promise<number | null> {
    await this.loadSlots()
    if (this.slotAvailable() === 0) return null
    const free = this.slotPorts!.findIndex(
      (port, index) => port != null && !this.busySlots.has(index),
    )
    if (free >= 0) {
      this.busySlots.add(free)
      return free
    }
    return new Promise<number | null>((resolve) => {
      this.slotWaiters.push((slot) => resolve(slot))
    })
  }

  private releaseSlot(slot: number | null) {
    if (slot == null) return
    this.busySlots.delete(slot)
    const next = this.slotWaiters.shift()
    if (next) {
      this.busySlots.add(slot)
      next(slot)
    }
  }

  /** 内核重启后槽端口可能变化:仅在无测试进行时清空,下次读取重新拉取。 */
  private refreshSlotsIfIdle() {
    if (this.busySlots.size === 0 && this.slotWaiters.length === 0) {
      this.slotPorts = null
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
    return entry ? { ...entry } : undefined
  }

  getSpeed(name: string, group: string) {
    const update = this.getSpeedUpdate(name, group)
    return update ? update.speed : -1
  }

  /** 行展示用:优先会话内实时值,其次持久化值(跨分组、刷新后仍可见)。 */
  getSpeedUpdateForMember(
    member: InteractableProxyMember | ResolvedProxyMember,
    group: string,
  ): SpeedUpdate | undefined {
    const live = this.getSpeedUpdate(member.ref.name, group)
    if (live) return live
    const saved = this.persisted.get(speedIdentityOfMember(member))
    return saved ? { ...saved } : undefined
  }

  getSpeedForMember(
    member: InteractableProxyMember | ResolvedProxyMember,
    group: string,
  ): number {
    const update = this.getSpeedUpdateForMember(member, group)
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
    this.refreshSlotsIfIdle()
    const update = await this.measureSpeed(
      member,
      group,
      url,
      windowSecs,
      maxBytes,
    )
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

    // 叶子节点优先走并行测速槽(绑定专用槽,不切 GLOBAL);子组成员或槽不可用时
    // 回落既有 GLOBAL 专用通道。两条路径的失败/超时语义一致。
    const slot = member.kind === 'node' ? await this.acquireSlot() : null

    try {
      // 前端兜底超时仅防异常时滞留测量中,归入超时态。
      const fallback = new Promise<number>((resolve) => {
        setTimeout(() => resolve(SPEED_TIMEOUT), windowSecs * 1000 + 10_000)
      })

      // maxBytes 归一为 null:serde Option 收到 None 即不限流量。
      const run =
        slot != null
          ? speedtestNodeViaSlot(
              slot,
              apiName,
              url,
              windowSecs,
              maxBytes ?? null,
            )
          : speedtestNode(apiName, url, windowSecs, maxBytes ?? null)
      const result = await Promise.race([run, fallback])

      let update: SpeedUpdate
      if (typeof result === 'number') {
        // 兜底超时触发:命令迟迟未返回,与后端下载错误的失败态区分。
        debugLog(`[SpeedManager] 前端兜底超时，代理: ${name}`)
        const elapsed = await padToMinDisplayMs(startTime)
        update = this.setSpeed(name, group, result, { elapsed })
      } else {
        const elapsed = await padToMinDisplayMs(startTime)
        debugLog(
          `[SpeedManager] 速度测试完成，代理: ${name}, 速度: ${result.speedBps} B/s`,
        )
        update = this.setSpeed(name, group, result.speedBps, { elapsed })
      }
      this.persistResult(member, update)
      return update
    } catch (error) {
      const elapsed = await padToMinDisplayMs(startTime)
      console.error(`[SpeedManager] 速度测试出错，代理: ${name}`, error)
      // 0 即失败态:后端下载错误/HTTP 非 2xx/窗口内 0 字节,不滞留测量中。
      const update = this.setSpeed(name, group, 0, { elapsed })
      this.persistResult(member, update)
      return update
    } finally {
      this.releaseSlot(slot)
    }
  }

  /** 整组测速:按并发数并行(1 = 逐个串行),受可用槽数限制。
   *
   *  每个叶子节点占一个测速槽、互不干扰;子组成员或槽不可用时回落 GLOBAL 通道,
   *  而 GLOBAL 切换是全局互斥的,因此只要存在非节点成员或没有任何可用槽,整批退化为串行,
   *  避免并发切换 GLOBAL 造成节点串味。单项通知即时、分组通知延后到收尾。 */
  async checkListSpeed(
    proxies: InteractableProxyMember[],
    group: string,
    url: string = DEFAULT_SPEEDTEST_URL,
    windowSecs: number = DEFAULT_SPEEDTEST_WINDOW_SECS,
    maxBytes?: number,
    options?: { concurrency?: number },
  ) {
    debugLog(
      `[SpeedManager] 批量测试速度开始，组: ${group}, 数量: ${proxies.length}`,
    )
    this.activeBatches.set(group, (this.activeBatches.get(group) ?? 0) + 1)
    proxies.forEach(({ ref }) => {
      this.setSpeed(ref.name, group, SPEED_TESTING)
    })

    const startTime = Date.now()

    this.refreshSlotsIfIdle()
    await this.loadSlots()
    const hasNonNode = proxies.some((member) => member.kind !== 'node')
    const slotCount = this.slotAvailable()
    // 并发数即并行开关:1 表示逐个串行;GLOBAL 通道互斥时也只能串行。
    const desired = Math.max(1, options?.concurrency ?? 1)
    const limit =
      slotCount === 0 || hasNonNode ? 1 : Math.min(desired, slotCount)

    try {
      let next = 0
      const worker = async () => {
        while (true) {
          const index = next++
          if (index >= proxies.length) return
          const member = proxies[index]
          const name = member.ref.name
          try {
            await this.measureSpeed(member, group, url, windowSecs, maxBytes)
          } catch (error) {
            // 单个节点的意外异常不拖垮整组;结果落失败态,继续下一个。
            console.error(
              `[SpeedManager] 批量测试单个代理出错，代理: ${name}`,
              error,
            )
            const update = this.setSpeed(name, group, 0)
            this.persistResult(member, update)
          }
        }
      }
      const workerCount = Math.min(limit, proxies.length)
      await Promise.all(
        Array.from({ length: Math.max(1, workerCount) }, () => worker()),
      )
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
