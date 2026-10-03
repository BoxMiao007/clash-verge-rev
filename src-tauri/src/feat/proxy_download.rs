//! 内核升级与 meow GEO 更新共用的逐出口探测下载(工单 06)。

use crate::utils::network::{NetworkManager, ProxyType};
use anyhow::{Result, anyhow};
use clash_verge_logging::{Type, logging};

/// 逐出口探测下载(Localhost → System → None),仅经代理可达 GitHub 时也能下载成功。
/// 下载不复用版本解析的出口:api.github.com 与 release-assets 走不同域名,同一时刻
/// 各出口对不同域名的可达性可能不同(实测代理出口被 API 限流、直连可达 API 却达不
/// 到资产下载域),复用解析出口会把可用的下载路线挡在门外(工单 06 实测)。
/// `scene` 仅作日志前缀,区分内核升级与 GEO 更新两条链路。
pub(super) async fn download_via_proxies(
    url: &str,
    timeout_secs: u64,
    max_bytes: usize,
    scene: &str,
) -> Result<Vec<u8>> {
    let mut last_error = None;
    for proxy in [ProxyType::Localhost, ProxyType::System, ProxyType::None] {
        match NetworkManager::new()
            .get_bytes(url, proxy, Some(timeout_secs), max_bytes)
            .await
        {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                logging!(debug, Type::Core, "{scene}: 经 {proxy:?} 下载 {url} 失败: {error:#}");
                last_error = Some(error.context(format!("{proxy:?} could not download {url}")));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("failed to download {url}")))
}
