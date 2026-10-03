//! 应用内内核升级(mihomo / mihomo-alpha / meow 三内核通用)。
//!
//! The core replaces itself by truncating its own running executable in place, which macOS
//! kills under Hardened Runtime and which leaves a 0-byte core behind when interrupted
//! (clash-verge-rev#6834). Verge downloads the release itself instead, stages a verified
//! copy beside the managed core and renames it into place: `rename` never touches the inode
//! the running core is executing from, so no page validation can fail.
//!
//! 更新源按当前内核分源(工单 06):mihomo 系走 MetaCubeX releases(version.txt + 扁平压缩包),
//! meow 走 meow-rs 官方 GitHub releases(GitHub API 取最新 tag + `meow-<tag>-<target>` 压缩包),
//! staging、发布、服务移交与回滚共用同一条路径。

use crate::{
    config::{Config, IVerge},
    core::{CoreManager, manager::RunningMode},
    utils::network::{NetworkManager, ProxyType},
};
use anyhow::{Context as _, Result, anyhow, bail};
use clash_verge_logging::{Type, logging};
use std::{
    env::current_exe,
    ffi::OsStr,
    fs::File,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const RELEASE_VERSION_URL: &str = "https://github.com/MetaCubeX/mihomo/releases/latest/download/version.txt";
const ALPHA_BASE_URL: &str = "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha";
const RELEASE_DOWNLOAD_URL: &str = "https://github.com/MetaCubeX/mihomo/releases/download";
const MEOW_LATEST_API_URL: &str = "https://api.github.com/repos/meow-rs/meow-rs/releases/latest";
const MEOW_DOWNLOAD_URL: &str = "https://github.com/meow-rs/meow-rs/releases/download";
const VERSION_TIMEOUT_SECS: u64 = 20;
const PACKAGE_TIMEOUT_SECS: u64 = 300;
/// Well above any real core package, low enough that a wrong response cannot exhaust memory.
const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;

/// meow release 资产的 target triple,按 (OS, ARCH) 查表(数据而非散落 if)。
/// 与 `scripts/prebuild.mjs` 的 `MEOW_ASSET_TARGETS` 是同一套事实的两个视图:那边以
/// host triple 为键,这边以 (OS, ARCH) 为键,逐行对应——fork 发布面只有 Windows x64
/// (zip),Linux x64 取 musl 静态二进制供开发自测;其余平台两表都不列,查不到即
/// fail fast(与 prebuild 对不受支持主机的报错一致)。下方测试锁住两表一致。
const MEOW_ASSET_TARGETS: &[(&str, &str, &str)] = &[
    // (std::env::consts::OS, std::env::consts::ARCH, 资产名里的 target triple)
    ("windows", "x86_64", "x86_64-pc-windows-msvc"),
    ("linux", "x86_64", "x86_64-unknown-linux-musl"),
];

static STAGING_GENERATION: AtomicU64 = AtomicU64::new(0);
/// `.rollback` and `.old` are fixed paths, so two upgrades must not overlap.
static UPGRADE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, serde::Serialize)]
pub struct CoreUpgradeReport {
    /// False when the managed core was already at the latest version.
    pub upgraded: bool,
    pub from: std::string::String,
    pub to: std::string::String,
}

pub async fn upgrade_core(force: bool) -> Result<CoreUpgradeReport> {
    let _serialized = UPGRADE_LOCK.lock().await;
    let core = Config::verge().await.latest_arc().get_valid_clash_core();
    tracing::Span::current().record("core", tracing::field::display(&core));
    let meow = IVerge::is_meow_core(&core);
    let alpha = !meow && core.ends_with("-alpha");
    let target = managed_core_path(&core)?;

    // A core already broken by the in-place updater cannot report a version; upgrading is the repair.
    let installed = read_core_version(&target).unwrap_or_else(|error| {
        logging!(warn, Type::Core, "core upgrade: unreadable installed core: {error:#}");
        std::string::String::new()
    });

    let (proxy, latest) = if meow {
        resolve_latest_meow_version().await?
    } else {
        resolve_latest_version(alpha).await?
    };
    let span = tracing::Span::current();
    span.record("from", tracing::field::display(&installed));
    span.record("to", tracing::field::display(&latest));
    logging!(debug, Type::Core, "core upgrade: latest version resolved via {proxy:?}");

    // meow 的 tag 与 `-v` 输出 v 前缀不一致,归一后再比(对 mihomo 无影响)。
    if !force && core_versions_match(&installed, &latest) {
        return Ok(CoreUpgradeReport {
            upgraded: false,
            from: installed,
            to: latest,
        });
    }

    let package = if meow {
        download_meow_package(&latest).await?
    } else {
        download_package(proxy, alpha, &latest).await?
    };
    let staged = stage_core(&target, &package, &latest, &core)?;

    publish_staged_core(staged, &target, &core, &installed).await?;

    Ok(CoreUpgradeReport {
        upgraded: true,
        from: installed,
        to: latest,
    })
}

/// staging 验证通过后的共同路径:硬链接保旧、原子发布、服务模式移交、重启,失败自动回滚。
async fn publish_staged_core(staged: StagedCore, target: &Path, core: &str, installed: &str) -> Result<()> {
    // A hard link keeps the previous core reachable without touching the inode the running
    // core executes from, so publishing below stays one atomic rename.
    let rollback = target.with_file_name(format!(".{core}.rollback"));
    let _ = std::fs::remove_file(&rollback);
    // Nothing to roll back to when the core we are replacing could not report a version.
    let restorable = !installed.is_empty() && std::fs::hard_link(target, &rollback).is_ok();

    // Decided before anything can crash: after a failed restart the mode reads NotRunning, which
    // says nothing about whether this upgrade went through the Service.
    let service_mode = matches!(*CoreManager::global().get_running_mode(), RunningMode::Service);

    let mut result = staged.publish(target);
    // The Service executes its own administrator-approved copy, never this file; hand the new
    // bytes over before the restart below asks for them, or a service-mode restart would keep
    // running the previous core while the app reports the new version. Sidecar mode runs the
    // file directly and gets no elevation prompt.
    let service_staging = if result.is_ok() && service_mode {
        result =
            crate::core::service::stage_approved_core(target).context("core replaced but not accepted by the service");
        if result.is_ok() {
            ServiceStaging::Succeeded
        } else {
            ServiceStaging::Refused
        }
    } else {
        ServiceStaging::NotAttempted
    };
    if result.is_ok() {
        result = CoreManager::global()
            .restart_core()
            .await
            .context("core replaced but failed to restart");
    }

    if let Err(error) = result {
        // Put the previous core back when its file is gone or its process is down. A failure
        // after the new core started must keep the new file instead. A service that refused the
        // new core also restores: leaving the new file would make a retry read the new version
        // and report nothing to do while the service keeps running the old core.
        // 重启失败的错误返回与内核退出簿记存在毫秒级竞态:立即读模式可能仍看到 Sidecar,
        // 把「新内核启动即死」误判为「新内核在跑」而跳过回滚,留下无内核运行的最糟状态
        // (工单 06 实测)。短暂沉降后再读,新内核若真在跑,模式仍是 Sidecar 且不改判定。
        let core_is_down = {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            matches!(*CoreManager::global().get_running_mode(), RunningMode::NotRunning)
        };
        if restorable
            && (matches!(service_staging, ServiceStaging::Refused) || core_is_down || !target.exists())
            && std::fs::rename(&rollback, target).is_ok()
        {
            logging!(warn, Type::Core, "core upgrade: rolled back to {installed:?}");
            // Only a staging that SUCCEEDED left the failing bytes in the approved copy, and only
            // then must the restore reach it too — otherwise the restart below would spawn the
            // very core that just failed. Refused or never-attempted staging left the approved
            // copy untouched, and re-staging would raise an elevation prompt for a no-op. This is
            // deliberately not gated on the current running mode, which reads NotRunning after
            // the very crash being rolled back.
            if matches!(service_staging, ServiceStaging::Succeeded)
                && let Err(stage_error) = crate::core::service::stage_approved_core(target)
            {
                logging!(
                    warn,
                    Type::Core,
                    "core upgrade: could not restore the service copy: {stage_error:#}"
                );
            }
            if core_is_down {
                let _ = CoreManager::global().restart_core().await;
            }
        }
        // Renaming one hard link over the other succeeds without consuming the name.
        let _ = std::fs::remove_file(&rollback);
        // Redundant once the rollback link exists; without one it is the only copy left.
        #[cfg(windows)]
        if restorable {
            let _ = std::fs::remove_file(target.with_extension("old"));
        }
        return Err(error);
    }
    let _ = std::fs::remove_file(&rollback);
    // The displaced core could not be deleted while it was still executing.
    #[cfg(windows)]
    let _ = std::fs::remove_file(target.with_extension("old"));

    Ok(())
}

/// Whether this upgrade handed bytes to the Service's approved core directory.
///
/// Three states matter, not two: a staging that never ran (publish failed, or sidecar mode) left
/// the approved copy alone just like a refused one, but only a SUCCEEDED staging obliges the
/// rollback path to restore that copy as well. A user who upgrades in sidecar mode and later
/// switches to service mode runs the previously approved core until the next elevated
/// (re)install or upgrade; the Service logs that drift on every spawn.
#[derive(Clone, Copy)]
enum ServiceStaging {
    NotAttempted,
    Succeeded,
    Refused,
}

/// The sidecar next to the app executable. On macOS development builds the Service runs a
/// staged copy of this file, so writing here is what makes an upgrade survive the next start.
fn managed_core_path(core: &str) -> Result<PathBuf> {
    let extension = if cfg!(windows) { ".exe" } else { "" };
    let path = current_exe()
        .context("failed to locate the current executable")?
        .with_file_name(format!("{core}{extension}"));
    Ok(path)
}

/// Pins the package to the resolved version, so a release moving on mid-upgrade cannot 404.
fn package_url(alpha: bool, version: &str) -> Result<std::string::String> {
    let asset = asset_base_name(alpha)?;
    let extension = if cfg!(windows) { "zip" } else { "gz" };
    Ok(if alpha {
        format!("{ALPHA_BASE_URL}/{asset}-{version}.{extension}")
    } else {
        format!("{RELEASE_DOWNLOAD_URL}/{version}/{asset}-{version}.{extension}")
    })
}

/// Mirrors the asset map in `scripts/prebuild.mjs` so an upgrade keeps the build variant
/// the bundled sidecar was taken from.
fn asset_base_name(alpha: bool) -> Result<&'static str> {
    let arch = std::env::consts::ARCH;
    let unsupported = || anyhow!("no mihomo release asset for {}-{arch}", std::env::consts::OS);

    let name = if cfg!(target_os = "windows") {
        match arch {
            "x86_64" => "mihomo-windows-amd64-v2",
            "x86" => "mihomo-windows-386",
            "aarch64" => "mihomo-windows-arm64",
            _ => return Err(unsupported()),
        }
    } else if cfg!(target_os = "macos") {
        match arch {
            "x86_64" if alpha => "mihomo-darwin-amd64-v1-go122",
            "x86_64" => "mihomo-darwin-amd64-v2-go122",
            "aarch64" => "mihomo-darwin-arm64-go122",
            _ => return Err(unsupported()),
        }
    } else {
        match arch {
            "x86_64" => "mihomo-linux-amd64-v2",
            "x86" => "mihomo-linux-386",
            "aarch64" => "mihomo-linux-arm64",
            "arm" => "mihomo-linux-armv7",
            "riscv64" => "mihomo-linux-riscv64",
            "loongarch64" => "mihomo-linux-loong64",
            _ => return Err(unsupported()),
        }
    };
    Ok(name)
}

/// Returns the proxy that reached GitHub so the package download reuses it.
#[tracing::instrument(skip_all, level = "debug", fields(alpha))]
async fn resolve_latest_version(alpha: bool) -> Result<(ProxyType, std::string::String)> {
    let url = if alpha {
        format!("{ALPHA_BASE_URL}/version.txt")
    } else {
        RELEASE_VERSION_URL.to_owned()
    };
    probe_until_usable(&url, VERSION_TIMEOUT_SECS, |body| {
        is_usable_version(body).then(|| body.to_owned())
    })
    .await
}

/// meow 的最新版本取自 GitHub releases/latest API 的 `tag_name`(形如 v0.21.2)。
/// meow 没有 mihomo 那样的 version.txt 资产,API 是唯一稳定的最新版事实源;
/// 未认证配额(60 次/时/IP)对人工触发的升级足够。
async fn resolve_latest_meow_version() -> Result<(ProxyType, std::string::String)> {
    probe_until_usable(MEOW_LATEST_API_URL, VERSION_TIMEOUT_SECS, meow_latest_tag).await
}

/// 解析 releases/latest 响应里的 `tag_name`;错误页、限流响应与越轨字符都不能变成版本号。
fn meow_latest_tag(body: &str) -> Option<std::string::String> {
    let tag = serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("tag_name")?
        .as_str()?
        .to_owned();
    is_usable_version(&tag).then_some(tag)
}

/// 逐出口探测直到响应体能给出可用值:即便只有应用自身代理可达 GitHub,升级也能工作。
/// 返回打通该 URL 的出口,供后续包下载复用。不可用的响应体(200 错误页、限流页)
/// 换下一个出口继续,而不是被当成版本号接受。
async fn probe_until_usable(
    url: &str,
    timeout_secs: u64,
    extract: impl Fn(&str) -> Option<std::string::String>,
) -> Result<(ProxyType, std::string::String)> {
    let mut last_error = None;

    for proxy in [ProxyType::Localhost, ProxyType::System, ProxyType::None] {
        let attempt = NetworkManager::new()
            .get(url, proxy, Some(timeout_secs), None, false)
            .await;

        match attempt {
            Ok(response) if response.status().is_success() => {
                // 200 应答的错误页不能变成版本号,更不能流进包 URL。
                let body = response.text().trim().to_owned();
                match extract(&body) {
                    Some(value) => return Ok((proxy, value)),
                    None => {
                        logging!(warn, Type::Core, "内核升级: {url} 返回了不可用的版本内容: {body}");
                        last_error = Some(anyhow!("{url} returned an unusable version"));
                    }
                }
            }
            Ok(response) => {
                logging!(
                    debug,
                    Type::Core,
                    "内核升级: 经 {proxy:?} 探测版本: 状态 {}",
                    response.status()
                );
                last_error = Some(anyhow!("{url} returned status {}", response.status()));
            }
            Err(error) => {
                logging!(debug, Type::Core, "内核升级: 经 {proxy:?} 探测版本失败: {error:#}");
                last_error = Some(error.context(format!("{proxy:?} could not reach {url}")));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("failed to read the latest core version from {url}")))
}

/// Keeps error pages, and anything that could steer the package URL, out of the version.
fn is_usable_version(version: &str) -> bool {
    version.starts_with(|c: char| c.is_ascii_alphanumeric())
        && version.len() <= 64
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// meow 的 release tag 带 `v` 前缀而 `-v` 输出没有(tag v0.21.2、`-v` 报 `Meow Meta 0.21.2`),
/// 比较前把前缀归一;mihomo 两边格式一致,归一对它不改变判定。
fn core_versions_match(a: &str, b: &str) -> bool {
    strip_release_v_prefix(a) == strip_release_v_prefix(b)
}

fn strip_release_v_prefix(version: &str) -> &str {
    version.strip_prefix('v').unwrap_or(version)
}

async fn download_package(proxy: ProxyType, alpha: bool, version: &str) -> Result<Vec<u8>> {
    let url = package_url(alpha, version)?;
    logging!(info, Type::Core, "core upgrade: downloading {url}");

    NetworkManager::new()
        .get_bytes(&url, proxy, Some(PACKAGE_TIMEOUT_SECS), MAX_PACKAGE_BYTES)
        .await
        .with_context(|| format!("failed to download {url}"))
}

/// 查当前运行平台对应的 meow release 资产 target triple。
fn meow_asset_target() -> Result<&'static str> {
    MEOW_ASSET_TARGETS
        .iter()
        .find(|(os, arch, _)| *os == std::env::consts::OS && *arch == std::env::consts::ARCH)
        .map(|(_, _, target)| *target)
        .ok_or_else(|| {
            anyhow!(
                "no meow release asset for {}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        })
}

/// 把 meow 包钉到已解析的版本:`meow-<tag>-<target>.zip`(Windows)
/// 或 `.tar.gz`(其余),与 `scripts/prebuild.mjs` 下载的是同一份资产。
fn meow_package_url(version: &str) -> Result<std::string::String> {
    let target = meow_asset_target()?;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    Ok(format!(
        "{MEOW_DOWNLOAD_URL}/{version}/meow-{version}-{target}.{extension}"
    ))
}

/// meow 包下载与 GEO 更新共用逐出口探测通道(缘由见 `proxy_download`);包体积上限
/// 与超时沿用 mihomo 包下载的同一组常量。
async fn download_meow_package(version: &str) -> Result<Vec<u8>> {
    let url = meow_package_url(version)?;
    logging!(info, Type::Core, "内核升级: 下载 {url}");

    super::proxy_download::download_via_proxies(&url, PACKAGE_TIMEOUT_SECS, MAX_PACKAGE_BYTES, "内核升级").await
}

/// A staged core that is removed unless publishing renamed it away.
struct StagedCore {
    path: PathBuf,
}

impl Drop for StagedCore {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => logging!(
                warn,
                Type::Core,
                "core upgrade: failed to clean {}: {error}",
                self.path.display()
            ),
        }
    }
}

/// Unpacks, permits, signs and runs the new core before it is allowed anywhere near the
/// live path, so a bad download can never replace a working core.
fn stage_core(target: &Path, package: &[u8], version: &str, core: &str) -> Result<StagedCore> {
    let directory = target
        .parent()
        .with_context(|| format!("the managed core has no parent directory: {}", target.display()))?;
    let core_name = target
        .file_name()
        .with_context(|| format!("the managed core has no file name: {}", target.display()))?;

    let (path, mut file) = create_staging_file(directory, core_name)?;
    let staged = StagedCore { path };

    unpack(package, &mut file, core).with_context(|| format!("failed to unpack the core package for {version}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mut permissions = file
            .metadata()
            .with_context(|| format!("failed to inspect {}", staged.path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        file.set_permissions(permissions)
            .with_context(|| format!("failed to make {} executable", staged.path.display()))?;
    }

    file.sync_all()
        .with_context(|| format!("failed to flush {}", staged.path.display()))?;
    drop(file);

    let staged_version = read_core_version(&staged.path).context("the staged core is not runnable")?;
    if !core_versions_match(&staged_version, version) {
        bail!("the staged core reports {staged_version}, expected {version}");
    }

    Ok(staged)
}

fn create_staging_file(directory: &Path, core_name: &OsStr) -> Result<(PathBuf, File)> {
    for _ in 0..32 {
        let generation = STAGING_GENERATION.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".{}.{}.{generation}.tmp",
            core_name.to_string_lossy(),
            std::process::id()
        ));

        match File::options().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).with_context(|| format!("failed to create {}", path.display()));
            }
        }
    }

    bail!("failed to create a unique staging file in {}", directory.display())
}

/// 两种内核的压缩包布局不同:mihomo 是扁平单文件(gz/zip),meow 把二进制放在
/// `meow-<tag>-<target>/` 一层目录下(unix 为 tar.gz)。按当前升级的内核分派。
fn unpack(package: &[u8], out: &mut File, core: &str) -> Result<()> {
    if IVerge::is_meow_core(core) {
        unpack_meow(package, out)
    } else {
        unpack_mihomo(package, out)
    }
}

#[cfg(windows)]
fn unpack_mihomo(package: &[u8], out: &mut File) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(package)).context("invalid core archive")?;
    let mut entry = archive.by_index(0).context("the core archive is empty")?;
    std::io::copy(&mut entry, out).context("failed to extract the core")?;
    Ok(())
}

#[cfg(not(windows))]
fn unpack_mihomo(package: &[u8], out: &mut File) -> Result<()> {
    let mut decoder = flate2::read::GzDecoder::new(package);
    std::io::copy(&mut decoder, out).context("failed to decompress the core")?;
    Ok(())
}

/// meow 的 tar.gz 把二进制放在 `meow-<tag>-<target>/` 目录层里,按文件名取条目,
/// 不能像 mihomo 那样直接取第一个条目(压缩包里还有 LICENSE/README)。
#[cfg(not(windows))]
fn unpack_meow(package: &[u8], out: &mut File) -> Result<()> {
    let gz = flate2::read::GzDecoder::new(package);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries().context("invalid meow core archive")? {
        let mut entry = entry.context("failed to read the meow core archive")?;
        let is_binary = entry
            .path()
            .ok()
            .and_then(|path| path.file_name().map(|name| name == OsStr::new("meow")))
            .unwrap_or(false);
        if is_binary {
            return std::io::copy(&mut entry, out)
                .map(|_| ())
                .context("failed to extract the meow core");
        }
    }
    bail!("the meow core archive has no `meow` binary")
}

/// Windows zip 同样带 `meow-<tag>-<target>/` 目录层(还含 wintun.dll),按文件名取 meow.exe。
#[cfg(windows)]
fn unpack_meow(package: &[u8], out: &mut File) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(package)).context("invalid meow core archive")?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .context("failed to read the meow core archive")?;
        let is_binary = Path::new(entry.name())
            .file_name()
            .map(|name| name == OsStr::new("meow.exe"))
            .unwrap_or(false);
        if is_binary {
            return std::io::copy(&mut entry, out)
                .map(|_| ())
                .context("failed to extract the meow core");
        }
    }
    bail!("the meow core archive has no meow.exe")
}

impl StagedCore {
    #[cfg(not(windows))]
    fn publish(self, target: &Path) -> Result<()> {
        std::fs::rename(&self.path, target)
            .with_context(|| format!("failed to publish the core to {}", target.display()))
    }

    #[cfg(windows)]
    fn publish(self, target: &Path) -> Result<()> {
        if std::fs::rename(&self.path, target).is_ok() {
            return Ok(());
        }

        // A running executable cannot be replaced on Windows, but it can be moved aside.
        let displaced = target.with_extension("old");
        let _ = std::fs::remove_file(&displaced);
        std::fs::rename(target, &displaced)
            .with_context(|| format!("failed to move the running core aside from {}", target.display()))?;

        if let Err(error) = std::fs::rename(&self.path, target) {
            // Say where the old core went when it cannot be put back either.
            return match std::fs::rename(&displaced, target) {
                Ok(()) => Err(error).with_context(|| format!("failed to publish the core to {}", target.display())),
                Err(restore) => Err(error).with_context(|| {
                    format!(
                        "failed to publish the core to {} and to restore it from {}: {restore}",
                        target.display(),
                        displaced.display()
                    )
                }),
            };
        }

        let _ = std::fs::remove_file(&displaced);
        Ok(())
    }
}

/// Keeps the `-v` probe and codesign from flashing a console window.
#[cfg(windows)]
fn new_command(program: impl AsRef<OsStr>) -> Command {
    use std::os::windows::process::CommandExt as _;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(windows))]
fn new_command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

/// 从 `-v` 输出的第三个 token 解析版本号:mihomo 是 `Mihomo Meta v1.19.30 darwin arm64 ...`,
/// meow 是 `Meow Meta 0.21.2`(mihomo 带 v 前缀而 meow 没有,比较时归一)。
fn read_core_version(path: &Path) -> Result<std::string::String> {
    let output = new_command(path)
        .arg("-v")
        .output()
        .with_context(|| format!("failed to run {} -v", path.display()))?;

    if !output.status.success() {
        bail!("{} -v exited with {}", path.display(), output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .nth(2)
        .map(str::to_owned)
        .with_context(|| format!("unexpected version output from {}: {stdout}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        core_versions_match, is_usable_version, meow_asset_target, meow_latest_tag, meow_package_url, package_url,
    };

    #[test]
    fn only_plain_version_tokens_reach_the_package_url() {
        assert!(is_usable_version("v1.19.30"));
        assert!(is_usable_version("alpha-c0e43eb"));
        for rejected in [
            "",
            ".",
            "..",
            "/etc/passwd",
            "v1.19.30 extra",
            "<html>nope</html>",
            &"v".repeat(65),
        ] {
            assert!(!is_usable_version(rejected), "accepted {rejected:?}");
        }
    }

    #[test]
    fn package_urls_pin_the_resolved_version() {
        let release = package_url(false, "v1.19.30").unwrap_or_default();
        let alpha = package_url(true, "alpha-c0e43eb").unwrap_or_default();
        assert!(release.contains("/releases/download/v1.19.30/"), "{release}");
        assert!(
            release.ends_with("-v1.19.30.gz") || release.ends_with("-v1.19.30.zip"),
            "{release}"
        );
        assert!(alpha.contains("/Prerelease-Alpha/"), "{alpha}");
    }

    #[test]
    fn version_comparison_ignores_the_release_v_prefix() {
        // meow 的 release tag 带 v 前缀,`-v` 输出没有(tag v0.21.2、-v 报 0.21.2)。
        assert!(core_versions_match("v0.21.2", "0.21.2"));
        assert!(core_versions_match("0.21.2", "v0.21.2"));
        // mihomo 两边格式一致,归一比较必须不改变判定。
        assert!(core_versions_match("v1.19.30", "v1.19.30"));
        assert!(core_versions_match("alpha-c0e43eb", "alpha-c0e43eb"));
        assert!(!core_versions_match("v0.21.2", "v0.21.1"));
        assert!(!core_versions_match("v0.21.2", "alpha-c0e43eb"));
    }

    #[test]
    fn meow_latest_tag_is_taken_from_the_api_json_body() {
        // 结构照抄 api.github.com/repos/meow-rs/meow-rs/releases/latest 的响应。
        let body = r#"{"url":"https://api.github.com/repos/meow-rs/meow-rs/releases/377650398","tag_name":"v0.21.2","name":"v0.21.2"}"#;
        assert_eq!(meow_latest_tag(body).as_deref(), Some("v0.21.2"));
        // 限流响应与错误页都必须被拒之门外,不能变成版本号。
        assert_eq!(meow_latest_tag(r#"{"message":"API rate limit exceeded"}"#), None);
        assert_eq!(meow_latest_tag("<html>nope</html>"), None);
        // 带 v 前缀以外的杂字符不符合可用版本判定。
        assert_eq!(meow_latest_tag(r#"{"tag_name":"../etc/passwd"}"#), None);
    }

    #[test]
    fn meow_asset_map_stays_in_lockstep_with_the_prebuild_map() {
        // 与 scripts/prebuild.mjs 的 MEOW_ASSET_TARGETS 逐行一致(键空间不同:那边以
        // host triple 为键,这里以 (OS, ARCH) 为键)。prebuild 表无法被 Rust 测试直读,
        // 快照即契约:两表必须同时增删,否则安装包与升级会拿到不同变体。
        let expected = [
            (("windows", "x86_64"), "x86_64-pc-windows-msvc"),
            (("linux", "x86_64"), "x86_64-unknown-linux-musl"),
        ];
        for ((os, arch), target) in expected {
            assert_eq!(
                super::MEOW_ASSET_TARGETS
                    .iter()
                    .find(|(entry_os, entry_arch, _)| (*entry_os, *entry_arch) == (os, arch))
                    .map(|(_, _, target)| *target),
                Some(target),
                "missing meow asset mapping for {os}-{arch}"
            );
        }
        // 表恰好只有发布面两行:多出的行等于声称支持 prebuild 不支持的平台。
        assert_eq!(
            super::MEOW_ASSET_TARGETS.len(),
            expected.len(),
            "MEOW_ASSET_TARGETS must stay in lockstep with scripts/prebuild.mjs"
        );
        // 同一 (os, arch) 只能有一行,否则查找结果取决于行序。
        let mut keys: Vec<_> = super::MEOW_ASSET_TARGETS
            .iter()
            .map(|(os, arch, _)| (*os, *arch))
            .collect();
        keys.sort_unstable();
        let total = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), total, "duplicate (os, arch) mapping in MEOW_ASSET_TARGETS");
    }

    #[test]
    fn meow_asset_target_resolves_for_the_running_host() {
        // 当前平台必须能查到资产:开发自测(linux)与发布(windows)都不能落空。
        let target = meow_asset_target().expect("host platform must map to a meow asset");
        assert!(target.contains(std::env::consts::ARCH), "{target}");
    }

    #[test]
    fn meow_package_url_pins_the_resolved_version() {
        let url = meow_package_url("v0.21.2").expect("host platform must map to a meow asset");
        // 资产名与官方 release 完全一致(本仓库实测清单):meow-<tag>-<target>.zip|tar.gz。
        #[cfg(windows)]
        let expected =
            "https://github.com/meow-rs/meow-rs/releases/download/v0.21.2/meow-v0.21.2-x86_64-pc-windows-msvc.zip";
        #[cfg(not(windows))]
        let expected = "https://github.com/meow-rs/meow-rs/releases/download/v0.21.2/meow-v0.21.2-x86_64-unknown-linux-musl.tar.gz";
        assert_eq!(url, expected);
    }

    /// 在临时目录准备一个接收解包产物的文件,测试后清理整个目录。
    fn temp_unpack_sink(tag: &str) -> (std::path::PathBuf, std::fs::File) {
        let directory = std::env::temp_dir().join(format!("verge-unpack-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("failed to create the unpack test directory");
        let path = directory.join("staged");
        let file = std::fs::File::create(&path).expect("failed to create the unpack sink");
        (directory, file)
    }

    #[test]
    fn mihomo_unpack_still_takes_the_single_flat_payload() {
        use super::unpack;
        use std::io::Write as _;

        // 回归守卫(工单 06):mihomo 的 unix 包是裸 gz,整包就是二进制本身,不走 tar。
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(b"mihomo-binary-bytes").unwrap();
        let package = gz.finish().unwrap();

        let (directory, mut out) = temp_unpack_sink("mihomo");
        unpack(&package, &mut out, "verge-mihomo").expect("flat gz must unpack as before");
        drop(out);
        assert_eq!(std::fs::read(directory.join("staged")).unwrap(), b"mihomo-binary-bytes");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(not(windows))]
    #[test]
    fn meow_archive_unpacks_the_binary_from_its_versioned_directory() {
        use super::unpack;
        use std::io::Write as _;

        // 构造与官方布局一致的 tar.gz:`meow-<tag>-<target>/` 目录层,二进制前还有 LICENSE、
        // README——按「第一个条目」或裸 gz 解压都拿不到正确内容。
        let mut builder = tar::Builder::new(Vec::new());
        for (path, data) in [
            (
                "meow-v0.21.2-x86_64-unknown-linux-musl/LICENSE",
                "license text".as_bytes(),
            ),
            ("meow-v0.21.2-x86_64-unknown-linux-musl/README.md", b"readme".as_slice()),
            (
                "meow-v0.21.2-x86_64-unknown-linux-musl/meow",
                b"meow-binary-bytes".as_slice(),
            ),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, data).unwrap();
        }
        let tarball = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tarball).unwrap();
        let package = gz.finish().unwrap();

        let (directory, mut out) = temp_unpack_sink("meow");
        unpack(&package, &mut out, "verge-meow").expect("meow tar.gz must unpack the binary by name");
        drop(out);
        assert_eq!(std::fs::read(directory.join("staged")).unwrap(), b"meow-binary-bytes");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
