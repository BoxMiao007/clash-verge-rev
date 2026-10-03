//! meow 下的 GEO 数据更新(工单 06)。
//!
//! mihomo 的「更新 GEO 数据」走内核 `/configs/geo` API,meow 刻意不实现该接口
//! (见 meow 仓库 `docs/mihomo-api-compatibility.md` 的刻意缺口清单),由 fork 侧补齐:
//! 按 meow 启动时自动下载的同源地址拉取三份文件,替换 `-d` 目录下的同名文件后重启内核生效。

use crate::{
    config::{Config, IVerge},
    core::CoreManager,
    utils::{
        dirs,
        network::{NetworkManager, ProxyType},
    },
};
use anyhow::{Context as _, Result, anyhow};
use clash_verge_logging::{Type, logging};
use std::path::PathBuf;

/// 与 meow 启动时自动下载完全同源同名的三份文件(spike 实证,工单 01):
/// (`-d` 目录下的文件名, 下载地址)。同源同名保证与 mihomo 所用格式一致,
/// 且重启后 meow 不会因「文件缺失」而自行重新下载。
const GEO_SOURCES: &[(&str, &str)] = &[
    (
        "Country.mmdb",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/country.mmdb",
    ),
    (
        "GeoLite2-ASN.mmdb",
        "https://github.com/P3TERX/GeoLite.mmdb/releases/latest/download/GeoLite2-ASN.mmdb",
    ),
    (
        "geosite.dat",
        "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/geosite.dat",
    ),
];

const GEO_DOWNLOAD_TIMEOUT_SECS: u64 = 120;
/// 远大于真实单文件(Country.mmdb ~7MB / ASN ~12MB / geosite ~4MB),挡住错误页耗尽内存。
const MAX_GEO_FILE_BYTES: usize = 64 * 1024 * 1024;

/// 两次 GEO 更新不得并发:staging 与替换都走固定路径。
static GEO_UPDATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn update_meow_geo() -> Result<()> {
    let _serialized = GEO_UPDATE_LOCK.lock().await;
    let core = Config::verge().await.latest_arc().get_valid_clash_core();
    // 该命令是 meow 专有通道;mihomo 走内核 API,误调在这里 fail fast。
    anyhow::ensure!(
        IVerge::is_meow_core(&core),
        "the GEO file update runs under the meow core only; mihomo updates via its own API"
    );

    let directory = dirs::app_home_dir()?;
    // 先全部下载到 `-d` 目录内的临时文件:任一失败即中止,现有 GEO 文件不被触碰。
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (name, url) in GEO_SOURCES {
        let bytes = download_via_proxies(url).await?;
        let temp = directory.join(format!(".{name}.update-tmp"));
        tokio::fs::write(&temp, &bytes)
            .await
            .with_context(|| format!("failed to write the staged GEO file {}", temp.display()))?;
        staged.push((temp, directory.join(name)));
    }

    // 与 meow 自动下载机制协调:停内核后再替换。meow 启动只在文件缺失时才自行下载,
    // 停止状态下完成替换可确保重启加载的就是 fork 侧更新的文件,不会被运行中下载覆盖。
    CoreManager::global()
        .stop_core()
        .await
        .context("GEO files downloaded but failed to stop the core")?;
    // 无论替换是否成功,内核都必须回到运行状态;替换错误在重启之后向上抛。
    let replace = replace_staged_files(&staged);
    CoreManager::global()
        .restart_core()
        .await
        .context("GEO files replaced but failed to restart the core")?;
    replace?;
    logging!(
        info,
        Type::Core,
        "meow GEO 更新: 已替换 {} 个文件并重启内核",
        staged.len()
    );
    Ok(())
}

/// 逐份原子替换(同目录 rename);任一失败立即中止,内核仍会先重启再上抛错误。
fn replace_staged_files(staged: &[(PathBuf, PathBuf)]) -> Result<()> {
    for (temp, target) in staged {
        std::fs::rename(temp, target)
            .with_context(|| format!("failed to replace {} with the downloaded file", target.display()))?;
    }
    Ok(())
}

/// 与内核升级同一条代理链(Localhost → System → None),保证仅经代理可达 GitHub 时仍可更新。
async fn download_via_proxies(url: &str) -> Result<Vec<u8>> {
    let mut last_error = None;
    for proxy in [ProxyType::Localhost, ProxyType::System, ProxyType::None] {
        match NetworkManager::new()
            .get_bytes(url, proxy, Some(GEO_DOWNLOAD_TIMEOUT_SECS), MAX_GEO_FILE_BYTES)
            .await
        {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                logging!(
                    debug,
                    Type::Core,
                    "meow GEO 更新: 经 {proxy:?} 下载 {url} 失败: {error:#}"
                );
                last_error = Some(error.context(format!("{proxy:?} could not download {url}")));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("failed to download {url}")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn geo_sources_match_meows_own_download_origins() {
        // 与 meow 启动自动下载完全同源同名(spike 实证记录的日志 URL):快照即契约,
        // 换源必须连同这份表与 meow 的下载行为一起复核。
        let expected = [
            (
                "Country.mmdb",
                "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/country.mmdb",
            ),
            (
                "GeoLite2-ASN.mmdb",
                "https://github.com/P3TERX/GeoLite.mmdb/releases/latest/download/GeoLite2-ASN.mmdb",
            ),
            (
                "geosite.dat",
                "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/geosite.dat",
            ),
        ];
        assert_eq!(super::GEO_SOURCES, expected);
        // 目标文件名唯一,替换互不覆盖。
        let mut names: Vec<_> = super::GEO_SOURCES.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total);
    }
}
