use super::{CmdResult, WithErrorCode as _};
use crate::feat;

use clash_verge_logging::{Type, logging};

/// 单节点下载测速:后端完成「切换 GLOBAL → 限时下载 → 恢复 GLOBAL」全过程。
///
/// url 与测速时长由前端传入(03 号工单接入设置后来自 verge 配置),
/// 失败/超时返回明确错误,由前端落入失败状态。
#[tauri::command]
pub async fn speedtest_node(name: String, url: String, duration_secs: u64) -> CmdResult<feat::SpeedTestResult> {
    feat::speedtest_node(name, url, duration_secs)
        .await
        .with_error_code("SPEEDTEST_FAILED")
}

/// 供前端查询专用测速 listener 端口是否就绪(调试/预检用)。
#[tauri::command]
pub async fn get_speedtest_listener_port() -> CmdResult<Option<u16>> {
    let runtime = crate::config::Config::runtime().await;
    let data = runtime.data_arc();
    let port = data.config.as_ref().and_then(feat::speedtest_listener_port);
    logging!(debug, Type::Cmd, "查询下载测速通道端口: {port:?}");
    Ok(port)
}
