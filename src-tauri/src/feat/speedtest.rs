//! 单节点下载测速(下载速度功能)。
//!
//! 机制见 docs/adr/0001-download-speedtest-via-global-listener.md:配置生成时注入
//! 一个仅绑定 127.0.0.1 的专用 mixed listener(`proxy: GLOBAL`,已由 01 号工单 spike
//! 实测验证);测速命令按「读取 GLOBAL 当前选择 → 写恢复日志 → PUT 切到被测节点 →
//! 从专用端口发起限时下载 → 返回前同步恢复 GLOBAL 并清日志」执行。不触碰用户分组的
//! 选择,不写 record_selected_node。进程被硬杀(kill -9/断电)导致的残留由内核启动
//! 检查兜底(`recover_pending_restore`),处置语义见 ADR 修订节。
//!
//! 已知取舍(ADR-0001):global 模式用户在测速窗口内流量会被波及;恢复 GLOBAL
//! 失败只记日志,不向用户报错(mihomo 会把 GLOBAL 选择持久化到 cache.db,因此
//! 超时/panic 路径也必须尝试恢复)。

use anyhow::{Context as _, Result, anyhow, bail};
use clash_verge_logging::{Type, logging};
use serde_yaml_ng::{Mapping, Value};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::core::handle::Handle;
use crate::utils::dirs::app_home_dir;

/// 专用 listener 在 mihomo 配置 `listeners` 中的唯一标记名。
pub const SPEEDTEST_LISTENER_NAME: &str = "verge-speedtest";
/// 专用 listener 的默认端口基准;被占用时向后顺延探测。
pub const SPEEDTEST_BASE_PORT: u16 = 9666;
/// 端口探测范围(mixed-port 被占时 mihomo 只记 error 不退出,必须主动避让)。
const PORT_PROBE_ATTEMPTS: u16 = 16;
/// 测速时长上下限(秒)。前端 `src/utils/speed.ts` 持有同一份界限(输入夹取与
/// 解析归一),跨语言无法共享常量,两处需同步修改;此处保留一份用于服务端校验。
const MIN_DURATION_SECS: u64 = 1;
const MAX_DURATION_SECS: u64 = 30;

/// 测速流量上限的合法区间(MB)。前端 `src/utils/speed.ts` 持有同一份界限做输入
/// 夹取与解析归一(「两处同步」约定,同测速时长);此处换算为字节做服务端校验。
const MB_BYTES: u64 = 1024 * 1024;
const MIN_SPEEDTEST_MAX_MB: u64 = 1;
const MAX_SPEEDTEST_MAX_MB: u64 = 1024;

/// IPC 返回给前端的结构:与仓库约定一致用驼峰键(Tauri 不改写命令返回值的
/// 字段名,前端按此契约读取,见下方契约测试)。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedTestResult {
    /// 窗口内实际收到的字节数(解压后)。
    pub bytes: u64,
    /// 实际用时(毫秒),含连接与 TTFB。
    pub elapsed_ms: u64,
    /// 速度 = 已下载字节 ÷ 实际用时(字节/秒)。
    pub speed_bps: u64,
}

/// 速度计算纯函数:字节 ÷ 用时,返回字节/秒。
///
/// elapsed 为零时按 1 毫秒计,避免除零;bytes 为零时速度为零。
pub fn calc_speed_bps(bytes: u64, elapsed: Duration) -> u64 {
    if bytes == 0 {
        return 0;
    }
    let millis = elapsed.as_millis().max(1);
    ((bytes as u128 * 1000) / millis) as u64
}

/// 构造专用 listener 的 mihomo 配置条目。
///
/// 仅绑定 127.0.0.1;`proxy: GLOBAL` 让该端口的流量跟随 GLOBAL 当前选中节点。
fn speedtest_listener_entry(port: u16) -> Mapping {
    let mut listener = Mapping::new();
    listener.insert("name".into(), SPEEDTEST_LISTENER_NAME.into());
    listener.insert("type".into(), "mixed".into());
    listener.insert("port".into(), port.into());
    listener.insert("listen".into(), "127.0.0.1".into());
    listener.insert("proxy".into(), "GLOBAL".into());
    listener
}

/// 专用 listener 是否为本模块注入的条目。
fn is_speedtest_listener(listener: &Mapping) -> bool {
    listener.get("name").and_then(Value::as_str) == Some(SPEEDTEST_LISTENER_NAME)
}

/// 从运行时配置中读出专用 listener 的端口。
pub fn speedtest_listener_port(config: &Mapping) -> Option<u16> {
    config
        .get("listeners")
        .and_then(Value::as_sequence)?
        .iter()
        .find_map(|item| {
            let listener = item.as_mapping()?;
            if !is_speedtest_listener(listener) {
                return None;
            }
            listener.get("port").and_then(|value| match value {
                Value::Number(num) => num.as_u64().map(|p| p as u16),
                Value::String(s) => s.parse().ok(),
                _ => None,
            })
        })
}

/// 收集配置里已被占用的端口,测速 listener 不得与之冲突。
fn collect_occupied_ports(config: &Mapping) -> Vec<u16> {
    let mut ports = Vec::new();
    let port_keys = ["mixed-port", "port", "socks-port", "redir-port", "tproxy-port"];
    for key in port_keys {
        if let Some(value) = config.get(key)
            && let Some(port) = value
                .as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| value.as_u64().map(|p| p as u16))
        {
            ports.push(port);
        }
    }

    if let Some(addr) = config.get("external-controller").and_then(Value::as_str)
        && let Some((_, port)) = addr.rsplit_once(':')
        && let Ok(port) = port.parse()
    {
        ports.push(port);
    }

    if let Some(listeners) = config.get("listeners").and_then(Value::as_sequence) {
        for item in listeners {
            if let Some(listener) = item.as_mapping()
                && !is_speedtest_listener(listener)
                && let Some(port) = listener.get("port").and_then(|value| value.as_u64().map(|p| p as u16))
            {
                ports.push(port);
            }
        }
    }

    ports
}

/// 探测一个未监听的回环端口。bind 后立即释放,存在理论上的 TOCTOU 窗口,
/// 但远好于固定端口撞车时 mihomo 静默失败。
fn probe_free_port(exclude: &[u16]) -> Option<u16> {
    for offset in 0..PORT_PROBE_ATTEMPTS {
        let port = SPEEDTEST_BASE_PORT + offset;
        if exclude.contains(&port) {
            continue;
        }
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if std::net::TcpListener::bind(addr).is_ok() {
            return Some(port);
        }
    }
    None
}

/// 向运行时配置注入专用测速 listener(幂等:重复注入只更新端口)。
///
/// 无可用端口时移除既有标记条目并记日志,测速命令会以"通道未就绪"失败。
pub fn inject_speedtest_listener(config: &mut Mapping) {
    let mut occupied = collect_occupied_ports(config);
    let Some(port) = probe_free_port(&occupied) else {
        logging!(warn, Type::Core, "下载测速:未找到可用端口,本次生成不注入测速 listener");
        remove_speedtest_listener(config);
        return;
    };

    // 探测成功后,本进程占用的端口已释放;从避让集合移除自身,避免重复注入时误判。
    occupied.retain(|&p| p != port);

    let entry = speedtest_listener_entry(port);
    if let Some(listeners) = config.get_mut("listeners").and_then(Value::as_sequence_mut) {
        // 幂等:替换旧标记条目,保留用户或其他模块的 listener。
        if let Some(existing) = listeners.iter_mut().find_map(|item| {
            let listener = item.as_mapping_mut()?;
            is_speedtest_listener(listener).then_some(listener)
        }) {
            *existing = entry;
        } else {
            listeners.push(Value::Mapping(entry));
        }
    } else {
        let listeners = vec![Value::Mapping(entry)];
        config.insert("listeners".into(), Value::Sequence(listeners));
    }

    logging!(debug, Type::Core, "下载测速:注入专用 listener,端口 {port}");
}

fn remove_speedtest_listener(config: &mut Mapping) {
    if let Some(listeners) = config.get_mut("listeners").and_then(Value::as_sequence_mut) {
        listeners.retain(|item| item.as_mapping().map(is_speedtest_listener) != Some(true));
    }
}

/// GLOBAL 恢复日志(崩溃安全):切换前把「原选择 → 被测节点」落盘,恢复成功后删除。
/// 进程被硬杀(kill -9/断电)时 Drop 守卫不会执行,而 mihomo 会把 GLOBAL 选择
/// 持久化到 cache.db,残留会跨会话存活;内核启动时按此日志把 GLOBAL 恢复原选择。
const RESTORE_JOURNAL_FILE: &str = "speedtest-global-restore.json";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct GlobalRestoreJournal {
    /// 切换前 GLOBAL 的原选择,即恢复目标。
    original: String,
    /// 本次测速切换到的节点(被测节点)。
    target: String,
}

fn restore_journal_path(dir: &Path) -> PathBuf {
    dir.join(RESTORE_JOURNAL_FILE)
}

/// 日志不存在或损坏时返回 None;损坏不删除,留待下次测速覆盖写入。
fn read_journal_file(dir: &Path) -> Option<GlobalRestoreJournal> {
    let text = std::fs::read_to_string(restore_journal_path(dir)).ok()?;
    match serde_json::from_str(&text) {
        Ok(journal) => Some(journal),
        Err(err) => {
            logging!(warn, Type::Core, "下载测速:恢复日志损坏,忽略: {err}");
            None
        }
    }
}

fn write_journal_file(dir: &Path, journal: &GlobalRestoreJournal) -> Result<()> {
    use std::io::Write as _;
    let path = restore_journal_path(dir);
    let tmp = restore_journal_path(dir).with_extension("tmp");
    let text = serde_json::to_string(journal).context("序列化恢复日志失败")?;
    // 临时文件 + sync_all + 同目录改名:断电时不至于读到半写的日志——cache.db 侧的
    // 残留可能已持久化,半写的日志救不回残留。目录项改名本身不做 fsync:跨平台成本
    // 高,极端断电窗口退化为「无日志」,与修复前行为一致。
    let mut file =
        std::fs::File::create(&tmp).with_context(|| format!("创建恢复日志临时文件失败: {}", tmp.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("写入恢复日志失败: {}", tmp.display()))?;
    file.sync_all()
        .with_context(|| format!("刷盘恢复日志失败: {}", tmp.display()))?;
    drop(file);
    std::fs::rename(&tmp, &path).with_context(|| format!("落盘恢复日志失败: {}", path.display()))
}

fn clear_journal_file(dir: &Path) {
    // 主文件与写一半时崩溃遗留的临时文件一并清理,二者不存在均视为已清理。
    for path in [
        restore_journal_path(dir),
        restore_journal_path(dir).with_extension("tmp"),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => logging!(warn, Type::Core, "下载测速:清除恢复日志失败: {err}"),
        }
    }
}

/// 内核启动时对恢复日志的处置决策(调用方保证日志存在且 GLOBAL 已可读;纯函数便于测试)。
#[derive(Debug, PartialEq, Eq)]
enum RestoreDecision {
    /// GLOBAL 已不在被测节点(用户已手动切换或残留被覆盖):仅清理日志不恢复,
    /// 避免覆盖用户的最新选择。
    ClearOnly,
    /// GLOBAL 仍停在被测节点:恢复到原选择。
    Restore(String),
}

fn decide_startup_restore(journal: &GlobalRestoreJournal, global_now: &str) -> RestoreDecision {
    if global_now == journal.target {
        RestoreDecision::Restore(journal.original.clone())
    } else {
        RestoreDecision::ClearOnly
    }
}

/// GLOBAL 选择读写的最小接缝:生产实现走 mihomo 插件客户端,测试注入假实现。
///
/// 方法显式返回 `impl Future + Send`:tauri 命令的 future 必须可跨线程轮询。
trait GlobalProxyOps: Send + Sync + 'static {
    fn global_now(&self) -> impl Future<Output = Result<Option<String>>> + Send;
    fn select_global(&self, node: &str) -> impl Future<Output = Result<()>> + Send;
}

/// 生产实现:经 Handle 的 mihomo 客户端读写 GLOBAL 组。
struct MihomoGlobalOps;

impl GlobalProxyOps for MihomoGlobalOps {
    async fn global_now(&self) -> Result<Option<String>> {
        Ok(Handle::mihomo()
            .get_proxy_by_name("GLOBAL")
            .await
            .map(|global| global.now)?)
    }

    async fn select_global(&self, node: &str) -> Result<()> {
        // 插件错误经 ? 归一到 anyhow::Error,与 trait 签名一致。
        Handle::mihomo().select_node_for_group("GLOBAL", node).await?;
        Ok(())
    }
}

/// 恢复 GLOBAL 选择:命令返回前的同步恢复与守卫 Drop 的兜底恢复共用此函数。
/// 失败只记日志,不向用户报错(mihomo 会把 GLOBAL 选择持久化到 cache.db,
/// panic/取消路径也必须尽力尝试还原);恢复成功才清除恢复日志,失败保留,
/// 交给内核启动时的恢复检查兜底。
async fn restore_global_selection<O: GlobalProxyOps>(ops: &O, node: &str, journal_dir: &Path) {
    match ops.select_global(node).await {
        Ok(()) => {
            clear_journal_file(journal_dir);
            logging!(debug, Type::Core, "下载测速:已恢复 GLOBAL 选择: {node}");
        }
        Err(err) => logging!(warn, Type::Core, "下载测速:恢复 GLOBAL 选择失败(仅记录): {err:#}"),
    }
}

/// GLOBAL 恢复守卫:正常路径由 [`GlobalSelectionGuard::restore`] 在命令返回前
/// 同步恢复并置位;Drop 仅在未置位时(panic 展开、恢复 future 被中途取消)派发
/// 后台恢复任务兜底——两条路径共用 [`restore_global_selection`]。
struct GlobalSelectionGuard<O: GlobalProxyOps> {
    ops: Option<O>,
    node: String,
    journal_dir: PathBuf,
    settled: bool,
}

impl<O: GlobalProxyOps> GlobalSelectionGuard<O> {
    /// 正常路径:同步等待恢复完成后置位,Drop 不再重复派发。
    async fn restore(mut self) {
        if let Some(ops) = self.ops.as_ref() {
            restore_global_selection(ops, &self.node, &self.journal_dir).await;
        }
        self.settled = true;
    }
}

impl<O: GlobalProxyOps> Drop for GlobalSelectionGuard<O> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // panic/提前返回的兜底路径:恢复无法被等待,派发后台任务尽力还原。
        let Some(ops) = self.ops.take() else {
            return;
        };
        let node = self.node.clone();
        let journal_dir = self.journal_dir.clone();
        tokio::spawn(async move { restore_global_selection(&ops, &node, &journal_dir).await });
    }
}

/// 核心流程:切到被测节点 → 限时下载 → 无条件恢复 GLOBAL,返回下载结果。
///
/// 正常路径在返回前同步 await 恢复完成:前端整组测速严格串行地逐个调用命令,
/// 若恢复是派发不等待的后台任务,下一节点命令的「PUT GLOBAL 到新节点」可能抢在
/// 恢复完成前执行,GLOBAL 最终状态顺序不定,后续节点会测到错误链路。
async fn speedtest_via_global<O: GlobalProxyOps>(
    ops: O,
    node: &str,
    journal_dir: &Path,
    download: impl Future<Output = Result<(u64, Duration)>> + Send,
) -> Result<(u64, Duration)> {
    let Some(original) = ops.global_now().await? else {
        bail!("无法读取 GLOBAL 当前选择,取消本次下载测速");
    };

    // GLOBAL 已是被测节点时无需切换,守卫也就无需恢复。
    let guard = if original == node {
        None
    } else {
        // 先落盘恢复日志再切换,切换成功后任何时刻崩溃都能据此恢复原选择;
        // 写入失败则放弃本次测速(fail-fast),不做无保护的切换。
        write_journal_file(
            journal_dir,
            &GlobalRestoreJournal {
                original: original.clone(),
                target: node.into(),
            },
        )?;
        if let Err(err) = ops.select_global(node).await {
            // 未切成功即无残留;立即清理,避免留下指向未发生切换的记录。
            clear_journal_file(journal_dir);
            return Err(err);
        }
        Some(GlobalSelectionGuard {
            ops: Some(ops),
            node: original,
            journal_dir: journal_dir.to_path_buf(),
            settled: false,
        })
    };

    let result = download.await;

    if let Some(guard) = guard {
        guard.restore().await;
    }
    result
}

fn validate_url(url: &str) -> Result<()> {
    let parsed = tauri::Url::parse(url)?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        bail!("测速 URL 仅支持 http/https: {url}");
    }
    Ok(())
}

fn clamp_duration(duration_secs: u64) -> u64 {
    duration_secs.clamp(MIN_DURATION_SECS, MAX_DURATION_SECS)
}

/// 归一测速流量上限:返回 Some(字节) 为有效上限,None 为不限。
///
/// 未传、0、非法值(负数、非整数、越出 1–1024 MB)一律视为不限,与前端解析
/// 语义一致(非法输入自动归一,坏配置不悄悄生效)。参数用 f64:前端 Number
/// 无整数概念,反序列化成整数类型会让非整数值直接报错而非归一。
fn normalize_max_bytes(max_bytes: Option<f64>) -> Option<u64> {
    let max_bytes = max_bytes?;
    if !max_bytes.is_finite() || max_bytes.fract() != 0.0 {
        return None;
    }
    let bytes = max_bytes as i64;
    let min = (MIN_SPEEDTEST_MAX_MB * MB_BYTES) as i64;
    let max = (MAX_SPEEDTEST_MAX_MB * MB_BYTES) as i64;
    (min..=max).contains(&bytes).then_some(bytes as u64)
}

/// 构建经专用 listener 端口转发的下载客户端。
fn build_download_client(port: u16) -> Result<reqwest::Client> {
    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))?;
    Ok(reqwest::Client::builder()
        .proxy(proxy)
        // 不设总超时:下载窗口由 deadline 自行控制。
        .build()?)
}

/// 限时下载:窗口内持续读取响应体,窗口耗尽、达到测速流量上限或 EOF 即止。
///
/// 返回 (字节数, 实际用时)。连接失败、响应错误、传输中断均返回 Err;
/// 窗口耗尽与达标截断都不是错误——已收到的字节参与计速,按实际数据计。
async fn timed_download(
    client: &reqwest::Client,
    url: &str,
    window: Duration,
    max_bytes: Option<u64>,
) -> Result<(u64, Duration)> {
    let start = Instant::now();
    let deadline = tokio::time::Instant::from_std(start + window);

    let response = match tokio::time::timeout_at(deadline, client.get(url).send()).await {
        Ok(result) => result?,
        Err(_) => {
            let elapsed = start.elapsed();
            // 窗口内连 TTFB 都没有:视为未收到数据,交由调用方落失败态。
            return Ok((0, elapsed));
        }
    };
    if !response.status().is_success() {
        bail!("测速下载响应异常: HTTP {}", response.status());
    }

    let mut bytes: u64 = 0;
    let mut response = response;
    loop {
        match tokio::time::timeout_at(deadline, response.chunk()).await {
            // 窗口耗尽:正常结束,已收字节参与计速。
            Err(_) => break,
            Ok(chunk) => {
                let Some(chunk) = chunk? else { break };
                bytes += chunk.len() as u64;
                // 达到流量上限:提前结束,不引入新的测量状态。末块可能略超上限,
                // 按实际收到字节计速,不回写截断值。
                if max_bytes.is_some_and(|cap| bytes >= cap) {
                    break;
                }
            }
        }
    }

    Ok((bytes, start.elapsed()))
}

/// 单节点下载测速:切换 GLOBAL → 限时下载 → 返回前无条件恢复 GLOBAL。
///
/// `max_bytes` 为可选的测速流量上限(前端由 MB 配置换算为字节传入);未传、0 或非法值
/// 归一为不限,行为与无上限时完全一致。达到上限提前结束并按实际数据计速。
pub async fn speedtest_node(
    name: String,
    url: String,
    duration_secs: u64,
    max_bytes: Option<f64>,
) -> Result<SpeedTestResult> {
    validate_url(&url)?;
    let window = Duration::from_secs(clamp_duration(duration_secs));
    let max_bytes = normalize_max_bytes(max_bytes);

    let runtime = Config::runtime().await;
    let data = runtime.data_arc();
    let config = data
        .config
        .as_ref()
        .ok_or_else(|| anyhow!("运行时配置尚未生成,无法执行下载测速"))?;
    let port = speedtest_listener_port(config).ok_or_else(|| anyhow!("下载测速通道未就绪,请重启内核后重试"))?;

    // 客户端构建在切换 GLOBAL 之前:失败时无需触发恢复。
    let client = build_download_client(port)?;
    let journal_dir = app_home_dir().context("无法定位应用数据目录,恢复日志无法落盘")?;
    let (bytes, elapsed) = speedtest_via_global(
        MihomoGlobalOps,
        &name,
        &journal_dir,
        timed_download(&client, &url, window, max_bytes),
    )
    .await?;

    if bytes == 0 {
        bail!(
            "窗口 {}ms 内未收到任何数据,节点可能不可用(用时 {}ms)",
            window.as_millis(),
            elapsed.as_millis()
        );
    }

    Ok(SpeedTestResult {
        bytes,
        elapsed_ms: elapsed.as_millis() as u64,
        speed_bps: calc_speed_bps(bytes, elapsed),
    })
}

/// 内核启动后的恢复检查:按恢复日志把崩溃残留的 GLOBAL 选择还原。
///
/// 处置语义见 [`decide_startup_restore`]:无日志零开销;残留已被覆盖仅清理;
/// 确需恢复时失败也放弃(节点可能已随订阅更新消失,不猜测用户意图)。
/// GLOBAL 读取失败时保留日志,留待下一次内核启动重试。
pub(crate) async fn recover_pending_restore() {
    let Ok(dir) = app_home_dir() else {
        logging!(warn, Type::Core, "下载测速:无法定位应用数据目录,跳过 GLOBAL 恢复检查");
        return;
    };
    recover_pending_restore_with(&MihomoGlobalOps, &dir).await;
}

async fn recover_pending_restore_with<O: GlobalProxyOps>(ops: &O, dir: &Path) {
    let Some(journal) = read_journal_file(dir) else {
        return;
    };
    let now = match ops.global_now().await {
        Ok(Some(now)) => now,
        other => {
            // GLOBAL 恒有当前选择,Ok(None) 与 Err 同属读取异常而非「已被用户覆盖」:
            // 保留日志,留待下一次内核启动重试。
            if let Err(err) = other {
                logging!(
                    warn,
                    Type::Core,
                    "下载测速:读取 GLOBAL 失败,保留恢复日志待下次启动重试: {err:#}"
                );
            } else {
                logging!(
                    warn,
                    Type::Core,
                    "下载测速:读取 GLOBAL 异常(无当前选择),保留恢复日志待下次启动重试"
                );
            }
            return;
        }
    };
    match decide_startup_restore(&journal, &now) {
        RestoreDecision::ClearOnly => {
            clear_journal_file(dir);
            logging!(debug, Type::Core, "下载测速:GLOBAL 残留已被覆盖,仅清理恢复日志");
        }
        RestoreDecision::Restore(original) => {
            match ops.select_global(&original).await {
                Ok(()) => {
                    clear_journal_file(dir);
                    logging!(info, Type::Core, "下载测速:已按恢复日志还原 GLOBAL 选择: {original}");
                }
                Err(err) => {
                    // 节点可能已随订阅更新消失;放弃恢复,残留可被下次测速或手动切换覆盖。
                    clear_journal_file(dir);
                    logging!(
                        warn,
                        Type::Core,
                        "下载测速:恢复 GLOBAL 失败,放弃并清理恢复日志: {err:#}"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IPC 契约:前端 src/services/speed.ts 读驼峰键 speedBps(Tauri 不改写
    /// 命令返回值的字段名),漏标 rename_all 会静默变成 undefined 而非报错。
    #[test]
    #[allow(clippy::unwrap_used)]
    fn ipc_payload_uses_camel_case_keys_expected_by_frontend() {
        let value = serde_json::to_value(SpeedTestResult {
            bytes: 1,
            elapsed_ms: 2,
            speed_bps: 3,
        })
        .unwrap();
        assert_eq!(value["bytes"], 1);
        assert_eq!(value["elapsedMs"], 2);
        assert_eq!(value["speedBps"], 3);
    }

    /// 归一语义的边界表:期望值用独立字面量(1MB = 1_048_576;1024MB = 1_073_741_824),
    /// 不引用限界常量,避免与实现互证。
    #[test]
    fn normalize_max_bytes_treats_missing_zero_and_invalid_as_unlimited() {
        assert_eq!(normalize_max_bytes(None), None, "未传即不限");
        assert_eq!(normalize_max_bytes(Some(0.0)), None, "0 表示不限");
        assert_eq!(normalize_max_bytes(Some(-1.0)), None, "负数归一为不限");
        assert_eq!(normalize_max_bytes(Some(1.5)), None, "非整数归一为不限");
        assert_eq!(
            normalize_max_bytes(Some(1_048_575.0)),
            None,
            "不足 1MB 视为越界,归一为不限"
        );
        assert_eq!(
            normalize_max_bytes(Some(1_073_741_825.0)),
            None,
            "超过 1024MB 视为越界,归一为不限"
        );
    }

    #[test]
    fn normalize_max_bytes_returns_byte_cap_for_legal_values() {
        assert_eq!(normalize_max_bytes(Some(1_048_576.0)), Some(1_048_576), "1MB 下界");
        assert_eq!(
            normalize_max_bytes(Some(10_485_760.0)),
            Some(10_485_760),
            "区间内取 10MB"
        );
        assert_eq!(
            normalize_max_bytes(Some(1_073_741_824.0)),
            Some(1_073_741_824),
            "1024MB 上界"
        );
    }

    #[test]
    fn speed_bps_is_bytes_divided_by_elapsed() {
        assert_eq!(calc_speed_bps(5000, Duration::from_secs(5)), 1000);
        // 不足整毫秒按 1ms 计,避免除零。
        assert_eq!(calc_speed_bps(1024, Duration::ZERO), 1_024_000);
        assert_eq!(calc_speed_bps(0, Duration::from_secs(5)), 0);
        // 大值不溢出:1GB/s 量级。
        assert_eq!(
            calc_speed_bps(1024 * 1024 * 1024, Duration::from_secs(1)),
            1024 * 1024 * 1024
        );
    }

    fn listener_entry(name: &str, port: u16) -> Value {
        let mut listener = Mapping::new();
        listener.insert("name".into(), name.into());
        listener.insert("type".into(), "mixed".into());
        listener.insert("port".into(), port.into());
        listener.insert("listen".into(), "0.0.0.0".into());
        Value::Mapping(listener)
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn injection_creates_loopback_global_listener_once() {
        let mut config = Mapping::new();
        inject_speedtest_listener(&mut config);

        let listeners = config.get("listeners").and_then(Value::as_sequence).unwrap();
        assert_eq!(listeners.len(), 1);
        let listener = listeners[0].as_mapping().unwrap();
        assert_eq!(
            listener.get("name").and_then(Value::as_str),
            Some(SPEEDTEST_LISTENER_NAME)
        );
        assert_eq!(listener.get("listen").and_then(Value::as_str), Some("127.0.0.1"));
        assert_eq!(listener.get("proxy").and_then(Value::as_str), Some("GLOBAL"));
        assert_eq!(listener.get("type").and_then(Value::as_str), Some("mixed"));

        // 幂等:重复注入不产生重复条目。
        inject_speedtest_listener(&mut config);
        let listeners = config.get("listeners").and_then(Value::as_sequence).unwrap();
        assert_eq!(listeners.len(), 1);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn injection_keeps_user_listeners_and_avoids_occupied_ports() {
        let mut config = Mapping::new();
        let user_port = SPEEDTEST_BASE_PORT;
        config.insert(
            "listeners".into(),
            Value::Sequence(vec![listener_entry("user-listener", user_port)]),
        );

        inject_speedtest_listener(&mut config);

        let listeners = config.get("listeners").and_then(Value::as_sequence).unwrap();
        assert_eq!(listeners.len(), 2);
        assert_eq!(
            listeners[0].as_mapping().unwrap().get("name").and_then(Value::as_str),
            Some("user-listener")
        );
        let injected = listeners[1].as_mapping().unwrap();
        let injected_port = injected.get("port").and_then(Value::as_u64).unwrap() as u16;
        assert_ne!(injected_port, user_port, "不得与既有 listener 端口冲突");
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn injected_port_is_readable() {
        let mut config = Mapping::new();
        assert_eq!(speedtest_listener_port(&config), None);

        inject_speedtest_listener(&mut config);
        let port = speedtest_listener_port(&config).unwrap();
        assert!(port >= SPEEDTEST_BASE_PORT);
    }

    #[test]
    fn user_listener_without_speedtest_marker_is_not_read() {
        let mut config = Mapping::new();
        config.insert(
            "listeners".into(),
            Value::Sequence(vec![listener_entry("other", 12345)]),
        );
        assert_eq!(speedtest_listener_port(&config), None);
    }

    fn temp_test_dir(tag: &str) -> PathBuf {
        #[allow(clippy::unwrap_used)]
        {
            let dir = std::env::temp_dir().join(format!(
                "speedtest-journal-{}-{}-{tag}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }
    }

    /// 假 GLOBAL 操作:记录 select 调用序列,可注入延迟、global_now 失败
    /// 与从第 N 次起的 select 失败,用于验证恢复时序、恢复日志的写入/清理
    /// 与启动恢复的处置语义。
    struct FakeGlobalOps {
        now: Option<String>,
        fail_now: bool,
        select_delay: Duration,
        fail_select_from: Option<usize>,
        calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl GlobalProxyOps for FakeGlobalOps {
        async fn global_now(&self) -> Result<Option<String>> {
            if self.fail_now {
                return Err(anyhow!("注入的 global_now 失败"));
            }
            Ok(self.now.clone())
        }

        #[allow(clippy::unwrap_used)]
        fn select_global(&self, node: &str) -> impl Future<Output = Result<()>> + Send {
            let node = node.to_string();
            async move {
                tokio::time::sleep(self.select_delay).await;
                // 作用域收窄锁守卫:不跨语句持有,亦满足 significant_drop 收紧要求。
                let switched = {
                    let mut calls = self.calls.lock().unwrap();
                    if self.fail_select_from.is_some_and(|from| calls.len() >= from) {
                        false
                    } else {
                        calls.push(node);
                        true
                    }
                };
                if switched {
                    Ok(())
                } else {
                    Err(anyhow!("注入的 select 失败"))
                }
            }
        }
    }

    fn fake_ops(now: &str, select_delay: Duration) -> (FakeGlobalOps, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        fake_ops_full(now, select_delay, false, None)
    }

    fn fake_ops_full(
        now: &str,
        select_delay: Duration,
        fail_now: bool,
        fail_select_from: Option<usize>,
    ) -> (FakeGlobalOps, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            FakeGlobalOps {
                now: Some(now.into()),
                fail_now,
                select_delay,
                fail_select_from,
                calls: std::sync::Arc::clone(&calls),
            },
            calls,
        )
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn journal_file_roundtrip_and_clear() {
        let dir = temp_test_dir("roundtrip");
        assert!(read_journal_file(&dir).is_none(), "无日志文件按不存在处理");

        let journal = GlobalRestoreJournal {
            original: "a".into(),
            target: "b".into(),
        };
        write_journal_file(&dir, &journal).unwrap();
        assert_eq!(read_journal_file(&dir), Some(journal));

        clear_journal_file(&dir);
        assert!(read_journal_file(&dir).is_none());
        // 重复清除幂等。
        clear_journal_file(&dir);

        // 损坏内容按不存在处理,不阻塞后续写入。
        std::fs::write(restore_journal_path(&dir), "not json").unwrap();
        assert!(read_journal_file(&dir).is_none());
    }

    #[test]
    fn decide_startup_restore_distinguishes_residue_and_overwrite() {
        let journal = GlobalRestoreJournal {
            original: "a".into(),
            target: "b".into(),
        };

        assert_eq!(
            decide_startup_restore(&journal, "b"),
            RestoreDecision::Restore("a".into()),
            "GLOBAL 仍停在被测节点:恢复原选择"
        );
        assert_eq!(
            decide_startup_restore(&journal, "c"),
            RestoreDecision::ClearOnly,
            "残留已被覆盖:仅清理,不覆盖用户最新选择"
        );
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn command_writes_journal_before_switch_and_clears_after_restore() {
        let (ops, calls) = fake_ops("origin", Duration::ZERO);
        let dir = temp_test_dir("wal");
        let mid = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mid_in_task = std::sync::Arc::clone(&mid);
        let dir_for_task = dir.clone();

        let result = speedtest_via_global(ops, "node-a", &dir, async move {
            *mid_in_task.lock().unwrap() = read_journal_file(&dir_for_task);
            Ok((1000u64, Duration::from_millis(1)))
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(
            mid.lock().unwrap().as_ref(),
            Some(&GlobalRestoreJournal {
                original: "origin".into(),
                target: "node-a".into()
            }),
            "切换与下载期间恢复日志必须在盘上(先写后切)"
        );
        assert!(read_journal_file(&dir).is_none(), "恢复完成后日志必须清除");
        assert_eq!(*calls.lock().unwrap(), vec!["node-a".to_string(), "origin".to_string()]);
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn restore_failure_keeps_journal_for_startup_recovery() {
        // 第 1 次 select(切换)成功,第 2 次(恢复)失败:命令照常返回,
        // 日志保留,交给内核启动时的恢复检查兜底。
        let (ops, calls) = fake_ops_full("origin", Duration::ZERO, false, Some(1));
        let dir = temp_test_dir("restore-fail");

        let result = speedtest_via_global(ops, "node-a", &dir, async { Ok((1000u64, Duration::from_millis(1))) }).await;

        assert!(result.is_ok(), "恢复失败不改变命令结果(与既有语义一致)");
        assert_eq!(*calls.lock().unwrap(), vec!["node-a".to_string()]);
        assert_eq!(
            read_journal_file(&dir),
            Some(GlobalRestoreJournal {
                original: "origin".into(),
                target: "node-a".into()
            }),
            "恢复失败时日志必须保留"
        );
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn switch_failure_clears_journal() {
        let (ops, _calls) = fake_ops_full("origin", Duration::ZERO, false, Some(0));
        let dir = temp_test_dir("switch-fail");

        let result = speedtest_via_global(ops, "node-a", &dir, async { Ok((1000u64, Duration::from_millis(1))) }).await;

        assert!(result.is_err());
        assert!(read_journal_file(&dir).is_none(), "未切成功即无残留,日志应立即清理");
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn recovery_restores_residue_and_clears_journal() {
        let (ops, calls) = fake_ops("node-b", Duration::ZERO);
        let dir = temp_test_dir("recover");
        write_journal_file(
            &dir,
            &GlobalRestoreJournal {
                original: "node-a".into(),
                target: "node-b".into(),
            },
        )
        .unwrap();

        recover_pending_restore_with(&ops, &dir).await;

        assert_eq!(*calls.lock().unwrap(), vec!["node-a".to_string()], "必须恢复到原选择");
        assert!(read_journal_file(&dir).is_none());
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn recovery_clears_only_when_residue_already_overwritten() {
        // 用户崩溃后已手动切到 node-c:不能再恢复,仅清理日志。
        let (ops, calls) = fake_ops("node-c", Duration::ZERO);
        let dir = temp_test_dir("covered");
        write_journal_file(
            &dir,
            &GlobalRestoreJournal {
                original: "node-a".into(),
                target: "node-b".into(),
            },
        )
        .unwrap();

        recover_pending_restore_with(&ops, &dir).await;

        assert!(calls.lock().unwrap().is_empty(), "不得触碰用户的最新选择");
        assert!(read_journal_file(&dir).is_none());
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn recovery_gives_up_and_clears_when_restore_fails() {
        // 原选择节点已失效:恢复失败后放弃(不反复重试)并清理日志。
        let (ops, _calls) = fake_ops_full("node-b", Duration::ZERO, false, Some(0));
        let dir = temp_test_dir("give-up");
        write_journal_file(
            &dir,
            &GlobalRestoreJournal {
                original: "node-a".into(),
                target: "node-b".into(),
            },
        )
        .unwrap();

        recover_pending_restore_with(&ops, &dir).await;

        assert!(read_journal_file(&dir).is_none(), "失败也必须清理日志");
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn recovery_keeps_journal_when_global_unreadable() {
        // 内核未就绪读取 GLOBAL 失败:保留日志,下次内核启动重试。
        let (ops, _calls) = fake_ops_full("node-b", Duration::ZERO, true, None);
        let dir = temp_test_dir("unreadable");
        write_journal_file(
            &dir,
            &GlobalRestoreJournal {
                original: "node-a".into(),
                target: "node-b".into(),
            },
        )
        .unwrap();

        recover_pending_restore_with(&ops, &dir).await;

        assert!(read_journal_file(&dir).is_some(), "读不到 GLOBAL 时保留日志待下次重试");
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn recovery_keeps_journal_when_now_missing() {
        // GLOBAL 可读但没有当前选择:同为读取异常,不得按「已被覆盖」清理。
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let ops = FakeGlobalOps {
            now: None,
            fail_now: false,
            select_delay: Duration::ZERO,
            fail_select_from: None,
            calls: std::sync::Arc::clone(&calls),
        };
        let dir = temp_test_dir("now-missing");
        write_journal_file(
            &dir,
            &GlobalRestoreJournal {
                original: "node-a".into(),
                target: "node-b".into(),
            },
        )
        .unwrap();

        recover_pending_restore_with(&ops, &dir).await;

        assert!(calls.lock().unwrap().is_empty(), "不得凭异常状态触发恢复");
        assert!(read_journal_file(&dir).is_some(), "读取异常时保留日志待下次重试");
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn recovery_is_idle_without_journal() {
        let (ops, calls) = fake_ops("node-b", Duration::ZERO);
        let dir = temp_test_dir("idle");

        recover_pending_restore_with(&ops, &dir).await;

        assert!(calls.lock().unwrap().is_empty(), "无日志时零开销,不触碰 mihomo");
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn command_returns_only_after_slow_global_restore_completes() {
        // 每次 select 耗时 50ms:若恢复仍是派发不等待的后台任务(旧实现),
        // 命令返回时恢复尚未执行,序列会缺少最后一步,本测试即失败。
        let (ops, calls) = fake_ops("origin", Duration::from_millis(50));
        let dir = temp_test_dir("order");

        let result = speedtest_via_global(ops, "node-a", &dir, async { Ok((1000u64, Duration::from_millis(1))) }).await;

        assert!(result.is_ok());
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["node-a".to_string(), "origin".to_string()],
            "命令返回时 GLOBAL 必须已同步恢复到原选择"
        );
        assert!(read_journal_file(&dir).is_none(), "命令收尾后恢复日志必须已清除");
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn restore_completes_even_when_download_fails() {
        let (ops, calls) = fake_ops("origin", Duration::from_millis(50));
        let dir = temp_test_dir("download-fail");

        let result = speedtest_via_global(ops, "node-a", &dir, async { Err(anyhow!("下载失败")) }).await;

        assert!(result.is_err());
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["node-a".to_string(), "origin".to_string()],
            "下载失败也必须在返回前同步恢复 GLOBAL"
        );
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn no_switch_and_no_restore_when_global_already_on_node() {
        let (ops, calls) = fake_ops("node-a", Duration::from_millis(50));
        let dir = temp_test_dir("no-switch");

        let result = speedtest_via_global(ops, "node-a", &dir, async { Ok((1000u64, Duration::from_millis(1))) }).await;

        assert!(result.is_ok());
        assert!(calls.lock().unwrap().is_empty(), "无需切换也就无需恢复");
        assert!(read_journal_file(&dir).is_none(), "无需切换也就不写恢复日志");
    }

    /// 真实 mihomo 端到端(#[ignore],本地验证):
    /// cargo test -p clash-verge --lib feat::speedtest -- --ignored --nocapture
    /// 依赖 src-tauri/sidecar/verge-mihomo-*(prebuild 产物),缺失时跳过。
    #[tokio::test]
    #[ignore = "需要真实 mihomo 内核(sidecar),仅本地验证"]
    #[allow(clippy::unwrap_used)]
    async fn global_restore_journal_end_to_end_with_real_mihomo() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let Some(mihomo_bin) = std::fs::read_dir(manifest.join("sidecar"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                name.starts_with("verge-mihomo-") && !name.contains("alpha") && !name.contains("service")
            })
        else {
            println!("未找到 mihomo sidecar,跳过(先运行 scripts/prebuild.mjs)");
            return;
        };

        struct KillOnDrop(std::process::Child);
        impl Drop for KillOnDrop {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let workdir = temp_test_dir("e2e");
        let controller = probe_free_port(&[]).unwrap();
        std::fs::write(
            workdir.join("config.yaml"),
            format!(
                "mode: rule\nlog-level: WARNING\nexternal-controller: 127.0.0.1:{controller}\n\
                 proxies:\n\
                 \x20 - name: node-a\n\x20   type: http\n\x20   server: 127.0.0.1\n\x20   port: 1\n\
                 \x20 - name: node-b\n\x20   type: http\n\x20   server: 127.0.0.1\n\x20   port: 1\n\
                 \x20 - name: node-c\n\x20   type: http\n\x20   server: 127.0.0.1\n\x20   port: 1\n"
            ),
        )
        .unwrap();

        let child = KillOnDrop(
            std::process::Command::new(&mihomo_bin)
                .arg("-d")
                .arg(&workdir)
                .arg("-f")
                .arg(workdir.join("config.yaml"))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );

        let base = format!("http://127.0.0.1:{controller}");
        let client = reqwest::Client::new();
        let mut ready = false;
        for _ in 0..50 {
            if client
                .get(format!("{base}/version"))
                .send()
                .await
                .is_ok_and(|resp| resp.status().is_success())
            {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        assert!(ready, "mihomo 未在 10s 内就绪");

        // 直连真实内核 REST API 的 GlobalProxyOps 实现。
        struct RealOps {
            base: String,
            client: reqwest::Client,
        }
        impl GlobalProxyOps for RealOps {
            async fn global_now(&self) -> Result<Option<String>> {
                let value: serde_json::Value = self
                    .client
                    .get(format!("{}/proxies/GLOBAL", self.base))
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                Ok(value
                    .get("now")
                    .or_else(|| value.get("proxy").and_then(|proxy| proxy.get("now")))
                    .and_then(|now| now.as_str())
                    .map(str::to_string))
            }

            async fn select_global(&self, node: &str) -> Result<()> {
                self.client
                    .put(format!("{}/proxies/GLOBAL", self.base))
                    .json(&serde_json::json!({ "name": node }))
                    .send()
                    .await?
                    .error_for_status()?;
                Ok(())
            }
        }
        let ops = RealOps {
            base: base.clone(),
            client: client.clone(),
        };

        // 场景 A(核心):崩溃残留 —— 日志在盘、GLOBAL 停在被测节点 → 恢复原选择。
        write_journal_file(
            &workdir,
            &GlobalRestoreJournal {
                original: "node-a".into(),
                target: "node-b".into(),
            },
        )
        .unwrap();
        ops.select_global("node-b").await.unwrap();
        assert_eq!(ops.global_now().await.unwrap().as_deref(), Some("node-b"));
        recover_pending_restore_with(&ops, &workdir).await;
        assert_eq!(
            ops.global_now().await.unwrap().as_deref(),
            Some("node-a"),
            "崩溃残留必须被恢复到原选择"
        );
        assert!(read_journal_file(&workdir).is_none());

        // 场景 B:残留已被覆盖(用户手动切到 node-b)→ 只清日志不恢复。
        write_journal_file(
            &workdir,
            &GlobalRestoreJournal {
                original: "node-a".into(),
                target: "node-c".into(),
            },
        )
        .unwrap();
        ops.select_global("node-b").await.unwrap();
        recover_pending_restore_with(&ops, &workdir).await;
        assert_eq!(
            ops.global_now().await.unwrap().as_deref(),
            Some("node-b"),
            "不得覆盖用户的最新选择"
        );
        assert!(read_journal_file(&workdir).is_none());

        // 场景 C:原选择节点已消失 → 恢复失败放弃并清日志,GLOBAL 不动。
        write_journal_file(
            &workdir,
            &GlobalRestoreJournal {
                original: "gone-node".into(),
                target: "node-b".into(),
            },
        )
        .unwrap();
        recover_pending_restore_with(&ops, &workdir).await;
        assert_eq!(ops.global_now().await.unwrap().as_deref(), Some("node-b"));
        assert!(read_journal_file(&workdir).is_none());

        drop(child);
        let _ = std::fs::remove_dir_all(&workdir);
    }
}
