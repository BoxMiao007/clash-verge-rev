//! 单节点下载测速(下载速度功能)。
//!
//! 机制见 docs/adr/0001-download-speedtest-via-global-listener.md:配置生成时注入
//! 一个仅绑定 127.0.0.1 的专用 mixed listener(`proxy: GLOBAL`,已由 01 号工单 spike
//! 实测验证);测速命令按「读取 GLOBAL 当前选择 → PUT 切到被测节点 → 从专用端口
//! 发起限时下载 → 无条件恢复 GLOBAL」执行。不触碰用户分组的选择,不写
//! record_selected_node。
//!
//! 已知取舍(ADR-0001):global 模式用户在测速窗口内流量会被波及;恢复 GLOBAL
//! 失败只记日志,不向用户报错(mihomo 会把 GLOBAL 选择持久化到 cache.db,因此
//! 超时/panic 路径也必须尝试恢复)。

use anyhow::{Result, anyhow, bail};
use clash_verge_logging::{Type, logging};
use serde_yaml_ng::{Mapping, Value};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::core::handle::Handle;

/// 专用 listener 在 mihomo 配置 `listeners` 中的唯一标记名。
pub const SPEEDTEST_LISTENER_NAME: &str = "verge-speedtest";
/// 专用 listener 的默认端口基准;被占用时向后顺延探测。
pub const SPEEDTEST_BASE_PORT: u16 = 9666;
/// 端口探测范围(mixed-port 被占时 mihomo 只记 error 不退出,必须主动避让)。
const PORT_PROBE_ATTEMPTS: u16 = 16;
/// 测速时长上下限(秒)。前端 03 号工单接入设置前,取值由内置默认提供。
const MIN_DURATION_SECS: u64 = 1;
const MAX_DURATION_SECS: u64 = 30;

#[derive(Debug, Clone, serde::Serialize)]
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
    ((bytes as u128 * 1000) / millis as u128) as u64
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
        let mut listeners = Vec::new();
        listeners.push(Value::Mapping(entry));
        config.insert("listeners".into(), Value::Sequence(listeners));
    }

    logging!(debug, Type::Core, "下载测速:注入专用 listener,端口 {port}");
}

fn remove_speedtest_listener(config: &mut Mapping) {
    if let Some(listeners) = config.get_mut("listeners").and_then(Value::as_sequence_mut) {
        listeners.retain(|item| item.as_mapping().map(is_speedtest_listener) != Some(true));
    }
}

/// 恢复 GLOBAL 选择的守卫:无论测速成功、失败还是 panic,Drop 都会派发恢复任务。
struct GlobalSelectionGuard {
    node: String,
}

impl Drop for GlobalSelectionGuard {
    fn drop(&mut self) {
        let node = self.node.clone();
        tokio::spawn(async move {
            match Handle::mihomo().select_node_for_group("GLOBAL", &node).await {
                Ok(()) => logging!(debug, Type::Core, "下载测速:已恢复 GLOBAL 选择: {node}"),
                Err(err) => logging!(warn, Type::Core, "下载测速:恢复 GLOBAL 选择失败(仅记录): {err:#}"),
            }
        });
    }
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

/// 构建经专用 listener 端口转发的下载客户端。
fn build_download_client(port: u16) -> Result<reqwest::Client> {
    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))?;
    Ok(reqwest::Client::builder()
        .proxy(proxy)
        // 不设总超时:下载窗口由 deadline 自行控制。
        .build()?)
}

/// 限时下载:窗口内持续读取响应体,窗口耗尽或 EOF 即止。
///
/// 返回 (字节数, 实际用时)。连接失败、响应错误、传输中断均返回 Err;
/// 窗口耗尽不是错误——已收到的字节参与计速。
async fn timed_download(client: &reqwest::Client, url: &str, window: Duration) -> Result<(u64, Duration)> {
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
            }
        }
    }

    Ok((bytes, start.elapsed()))
}

/// 单节点下载测速:切换 GLOBAL → 限时下载 → 无条件恢复 GLOBAL。
pub async fn speedtest_node(name: String, url: String, duration_secs: u64) -> Result<SpeedTestResult> {
    validate_url(&url)?;
    let window = Duration::from_secs(clamp_duration(duration_secs));

    let runtime = Config::runtime().await;
    let data = runtime.data_arc();
    let config = data
        .config
        .as_ref()
        .ok_or_else(|| anyhow!("运行时配置尚未生成,无法执行下载测速"))?;
    let port = speedtest_listener_port(config).ok_or_else(|| anyhow!("下载测速通道未就绪,请重启内核后重试"))?;

    let mihomo = Handle::mihomo();
    let global = mihomo.get_proxy_by_name("GLOBAL").await?;
    let Some(original) = global.now else {
        bail!("无法读取 GLOBAL 当前选择,取消本次下载测速");
    };
    //GLOBAL 已是被测节点时无需切换,守卫也就无需恢复。
    let guard = if original == name {
        None
    } else {
        mihomo.select_node_for_group("GLOBAL", &name).await?;
        Some(GlobalSelectionGuard { node: original })
    };

    let client = build_download_client(port);
    let result = match client {
        Ok(client) => match timed_download(&client, &url, window).await {
            Ok((bytes, elapsed)) if bytes > 0 => Ok(SpeedTestResult {
                bytes,
                elapsed_ms: elapsed.as_millis() as u64,
                speed_bps: calc_speed_bps(bytes, elapsed),
            }),
            Ok((_, elapsed)) => Err(anyhow!(
                "窗口 {}ms 内未收到任何数据,节点可能不可用(用时 {}ms)",
                window.as_millis(),
                elapsed.as_millis()
            )),
            Err(err) => Err(err),
        },
        Err(err) => Err(err.into()),
    };

    // 守卫 Drop 恢复 GLOBAL;若本就是被测节点则无需恢复。
    drop(guard);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
