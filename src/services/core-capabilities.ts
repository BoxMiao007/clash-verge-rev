import { invoke } from '@tauri-apps/api/core'

/** 内核升级/GEO 更新的执行通道归属:内核自带接口,还是由 fork 侧命令托管。 */
export type CoreUpdateChannel = 'kernelApi' | 'forkSide'

/**
 * 内核能力:当前内核实现对外支持的操作与特性集合。
 * Rust 侧 `core/capability.rs` 是判定事实源;本模块是前端唯一消费点,
 * 组件从这里取事实做灰显与分流,不写 `内核 === 'verge-meow'` 之类的散落特判。
 */
export interface ICoreCapabilities {
  /** 当前内核标识(verge 侧 car 前缀惯例取值)。 */
  core: string
  /** 是否跟踪 UDP 连接:meow 不跟踪,连接页不会出现 UDP 会话。 */
  udpConnectionTracking: boolean
  /** 是否提供规则命中计数:meow 的 /rules 无命中数据。 */
  ruleHitCounting: boolean
  /** 是否支持监听器热替换:meow 改端口/监听类设置需内核重启才生效。 */
  listenerHotReload: boolean
  /** GEO 数据库更新的通道归属(工单 06 消费)。 */
  geoUpdateChannel: CoreUpdateChannel
  /** 内核升级的通道归属(两内核均由 fork 侧托管,见 ADR-0003;工单 06 消费)。 */
  coreUpgradeChannel: CoreUpdateChannel
}

/**
 * 查询未就绪时的 mihomo 基线:能力按"支持"处理,保证 mihomo 用户的界面
 * 在任何加载瞬间都不出现灰显;meow 的差异在载荷到达后才收敛呈现。
 */
export const MIHOMO_BASELINE: ICoreCapabilities = {
  core: 'verge-mihomo',
  udpConnectionTracking: true,
  ruleHitCounting: true,
  listenerHotReload: true,
  geoUpdateChannel: 'kernelApi',
  coreUpgradeChannel: 'forkSide',
}

export const normalizeCoreCapabilities = (
  raw?: ICoreCapabilities | null,
): ICoreCapabilities => raw ?? MIHOMO_BASELINE

/** 当前内核的显示名;内核命名映射的唯一前端消费点(use-clash 等)。 */
export const getCoreDisplayName = (core?: string): string =>
  core === 'verge-meow' ? 'Meow' : 'Mihomo'

export async function getCoreCapabilities(): Promise<ICoreCapabilities> {
  return invoke<ICoreCapabilities>('get_core_capabilities')
}
