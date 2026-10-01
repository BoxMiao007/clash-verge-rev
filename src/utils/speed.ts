/** 下载速度的语义归一、展示与排序比较。
 *
 * 速度值语义与延迟共用同一套测量状态(见 GLOSSARY.md):
 * -2 测量中;-3 超时(前端兜底时限内命令未返回);0 失败(下载失败/节点不通/窗口内无数据);
 * >0 有结果(字节/秒);其余负值视为未测试(-1)。
 */

export const SPEED_TESTING = -2

/** 超时哨兵:取负值与未测试族相邻,正数域全部保留给真实速度。 */
export const SPEED_TIMEOUT = -3

export type SpeedState =
  | 'testing'
  | 'untested'
  | 'timeout'
  | 'failed'
  | 'measured'

export const classifySpeed = (speed: number): SpeedState => {
  if (!Number.isFinite(speed)) return 'untested'
  if (speed === SPEED_TESTING) return 'testing'
  if (speed === SPEED_TIMEOUT) return 'timeout'
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
    case 'timeout':
      return 'Timeout'
    case 'failed':
      return 'Failed'
    case 'measured': {
      if (speed < MB) return `${Math.round(speed / KB)} KB/s`
      // 1 GB/s 以上仍以 MB/s 展示,量级足够直观且避免引入第三种单位。
      return `${(speed / MB).toFixed(1)} MB/s`
    }
  }
}

/** 展示配色:与延迟分色独立,速度越快越绿;超时与失败同延迟的 Timeout/Error 一样仅以文字区分。 */
export const formatSpeedColor = (speed: number): string => {
  switch (classifySpeed(speed)) {
    case 'untested':
    case 'testing':
      return ''
    case 'timeout':
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

/** 排序分档:有结果 > 超时 > 失败 > 测量中 > 未测试(与延迟的 rank 同序)。 */
const rankOf = (state: SpeedState): number => {
  switch (state) {
    case 'measured':
      return 0
    case 'timeout':
      return 1
    case 'failed':
      return 2
    case 'testing':
      return 3
    case 'untested':
      return 4
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

/** 测速时长允许范围(秒):下限保证有数据可算,上限限制整组测速的流量消耗。
 *  后端 src-tauri/src/feat/speedtest.rs 持有同一份界限做服务端校验,两处需同步修改。 */
export const MIN_SPEEDTEST_DURATION_SECS = 1
export const MAX_SPEEDTEST_DURATION_SECS = 30

/** 夹取测速时长到允许区间:设置输入框即时归一与配置解析共用。 */
export const clampSpeedtestDurationSecs = (secs: number): number =>
  Math.min(
    MAX_SPEEDTEST_DURATION_SECS,
    Math.max(MIN_SPEEDTEST_DURATION_SECS, secs),
  )

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
  return clampSpeedtestDurationSecs(configured)
}

/** 测速流量上限合法区间(MB):0 表示不限,1–1024 为有效上限。
 *  后端 src-tauri/src/feat/speedtest.rs 持有同一份界限做服务端校验,两处需同步修改。 */
export const MIN_SPEEDTEST_MAX_MB = 1
export const MAX_SPEEDTEST_MAX_MB = 1024

const BYTES_PER_MB = 1024 * 1024

/** 两解析函数共用的合法性判断:未配置、非整数或低于下限即非法;
 *  越上界的语义两函数不同(MB 夹到上界、字节不限),由各自函数处理。 */
const isValidMaxMb = (configured?: number | null): configured is number =>
  configured != null &&
  Number.isInteger(configured) &&
  configured >= MIN_SPEEDTEST_MAX_MB

/** 解析测速流量上限为展示/保存值(MB):未配置、负数、非整数归 0(不限),
 *  越界夹到上界。设置条目的展示与输入即时归一共用;与时长不同,0 是合法输入。 */
export const resolveSpeedtestMaxMb = (configured?: number | null): number => {
  if (!isValidMaxMb(configured)) {
    return 0
  }
  return Math.min(MAX_SPEEDTEST_MAX_MB, configured)
}

/** 解析测速流量上限(MB 配置)为按字节传入后端的参数:0/未配置/非法(负数、
 *  非整数、越界)一律不限(返回 undefined,不传参数),合法值由 MB 换算为字节。
 *  语义与后端 normalize_max_bytes 一致:非法输入自动归一,坏配置不悄悄生效。 */
export const resolveSpeedtestMaxBytes = (
  configured?: number | null,
): number | undefined => {
  if (!isValidMaxMb(configured) || configured > MAX_SPEEDTEST_MAX_MB) {
    return undefined
  }
  return configured * BYTES_PER_MB
}
