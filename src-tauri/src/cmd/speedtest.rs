use super::{CmdResult, WithErrorCode as _};
use crate::feat;

/// 单节点下载测速:后端完成「切换 GLOBAL → 限时下载 → 恢复 GLOBAL」全过程。
///
/// url 与测速时长由前端传入(03 号工单接入设置后来自 verge 配置);maxBytes 为
/// 可选的字节上限,未传或 0 不限,达到上限提前结束并按实际数据计速。
/// 下载错误/HTTP 非 2xx/窗口内 0 字节返回明确错误,由前端落入失败态;
/// 前端另有兜底超时,命令迟迟未返回时落入超时态。
#[tauri::command]
pub async fn speedtest_node(
    name: String,
    url: String,
    duration_secs: u64,
    max_bytes: Option<f64>,
) -> CmdResult<feat::SpeedTestResult> {
    feat::speedtest_node(name, url, duration_secs, max_bytes)
        .await
        .with_error_code("SPEEDTEST_FAILED")
}
