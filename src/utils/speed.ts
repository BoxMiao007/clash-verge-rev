/** 下载速度的语义归一、展示与排序比较。
 *
 * 速度值语义与延迟共用同一套测量状态(见 CONTEXT.md):
 * -2 测量中;0 失败(下载失败/节点不通/窗口内无数据);>0 有结果(字节/秒);
 * 其余负值视为未测试(-1)。
 */

export const SPEED_TESTING = -2

export type SpeedState = 'testing' | 'untested' | 'failed' | 'measured'

export const classifySpeed = (speed: number): SpeedState => {
  if (!Number.isFinite(speed)) return 'untested'
  if (speed === SPEED_TESTING) return 'testing'
  if (speed === 0) return 'failed'
  if (speed < 0) return 'untested'
  return 'measured'
}

const KB = 1024
const MB = 1024 * 1024

/** 展示文本:MB/s 一位小数,KB/s 整数(与延迟的 formatDelay 同风格,状态词保持原文)。 */
export const formatSpeed = (speed: number): string => {
  switch (classifySpeed(speed)) {
    case 'testing':
      return 'testing'
    case 'untested':
      return '-'
    case 'failed':
      return 'Failed'
    case 'measured': {
      if (speed < MB) return `${Math.round(speed / KB)} KB/s`
      // 1 GB/s 以上仍以 MB/s 展示,量级足够直观且避免引入第三种单位。
      return `${(speed / MB).toFixed(1)} MB/s`
    }
  }
}

/** 展示配色:与延迟分色独立,速度越快越绿。 */
export const formatSpeedColor = (speed: number): string => {
  switch (classifySpeed(speed)) {
    case 'untested':
    case 'testing':
      return ''
    case 'failed':
      return 'error.main'
    case 'measured': {
      const mbps = speed / MB
      if (mbps >= 5) return 'success.main'
      if (mbps >= 1) return 'primary.main'
      return 'warning.main'
    }
  }
}

/** 排序分档:有结果 > 测量中 > 失败 > 未测试。 */
const rankOf = (state: SpeedState): number => {
  switch (state) {
    case 'measured':
      return 0
    case 'testing':
      return 1
    case 'failed':
      return 2
    case 'untested':
      return 3
  }
}

/** 速度排序比较:降序(越大越前),非测量结果不分先后。 */
export const compareBySpeed = (a: number, b: number): number => {
  const rankDifference = rankOf(classifySpeed(a)) - rankOf(classifySpeed(b))
  if (rankDifference !== 0) return rankDifference

  if (classifySpeed(a) !== 'measured') return 0
  return b - a
}

/** 内置默认参数:verge 设置未配置或非法时的兜底值(设置项见 03 号工单)。 */
export const DEFAULT_SPEEDTEST_URL =
  'https://dl.google.com/chrome/mac/universal/stable/GGRO/googlechrome.dmg'
export const DEFAULT_SPEEDTEST_WINDOW_SECS = 5

/** 测速时长允许范围(秒):下限保证有数据可算,上限限制整组测速的流量消耗。 */
export const MIN_SPEEDTEST_DURATION_SECS = 1
export const MAX_SPEEDTEST_DURATION_SECS = 30

/** 解析测速 URL:空白视为未配置,回落内置默认。 */
export const resolveSpeedtestUrl = (configured?: string | null): string => {
  const trimmed = configured?.trim()
  return trimmed ? trimmed : DEFAULT_SPEEDTEST_URL
}

/** 解析测速时长(秒):非整数或超范围回落内置默认,夹在允许区间内。 */
export const resolveSpeedtestDurationSecs = (
  configured?: number | null,
): number => {
  if (
    configured == null ||
    !Number.isFinite(configured) ||
    !Number.isInteger(configured)
  ) {
    return DEFAULT_SPEEDTEST_WINDOW_SECS
  }
  return Math.min(
    MAX_SPEEDTEST_DURATION_SECS,
    Math.max(MIN_SPEEDTEST_DURATION_SECS, configured),
  )
}
