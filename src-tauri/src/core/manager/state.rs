#[cfg(test)]
use super::claim_core_readiness_generation;
use super::{CoreManager, PROFILE_SELECTIONS_PENDING_COMMIT, RunningMode};
use crate::{
    AsyncHandler,
    config::Config,
    core::{handle, logger::Logger, manager::CLASH_LOGGER, proxy_control, service},
    logging,
    utils::dirs,
};
use anyhow::{Context as _, Result};
use clash_verge_logging::Type;
use log::Level;
use std::path::Path;
use tauri_plugin_mihomo::MihomoExt as _;
use tauri_plugin_shell::ShellExt as _;

#[cfg(target_os = "windows")]
fn sidecar_config_without_tun(yaml: &str) -> Result<std::string::String> {
    use serde_yaml_ng::{Mapping, Value};

    let mut config: Mapping = serde_yaml_ng::from_str(yaml)?;
    let tun = config
        .entry(Value::from("tun"))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if !tun.is_mapping() {
        *tun = Value::Mapping(Mapping::new());
    }
    tun.as_mapping_mut()
        .context("invalid Sidecar TUN configuration")?
        .insert(Value::from("enable"), Value::Bool(false));
    Ok(serde_yaml_ng::to_string(&config)?)
}

const SIDECAR_READINESS_ATTEMPTS: usize = 30;

/// sidecar 启动时的 API 传输形态,按内核分流(工单 02):
/// mihomo 系监听 IPC socket(无 TCP 暴露);meow 只支持 TCP external-controller,
/// 地址复用 verge 的 `external-controller` 设置,密钥随行走 Bearer/`?token=`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SidecarApiTransport {
    IpcSocket {
        path: std::string::String,
    },
    Tcp {
        host: std::string::String,
        port: u16,
        secret: Option<std::string::String>,
    },
}

pub(super) fn sidecar_api_transport(
    core: &str,
    ipc_path: &str,
    tcp_controller: &str,
    secret: Option<&str>,
) -> anyhow::Result<SidecarApiTransport> {
    if crate::config::IVerge::is_meow_core(core) {
        let addr: std::net::SocketAddr = tcp_controller.parse().map_err(|error| {
            anyhow::anyhow!(
                "meow core requires a parsable external-controller address, got {tcp_controller:?}: {error}"
            )
        })?;
        return Ok(SidecarApiTransport::Tcp {
            host: addr.ip().to_string(),
            port: addr.port(),
            secret: secret.filter(|s| !s.is_empty()).map(Into::into),
        });
    }
    Ok(SidecarApiTransport::IpcSocket { path: ipc_path.into() })
}

/// 把 sidecar 的传输形态同步到 mihomo API 客户端。两个分支都显式设置协议,
/// 防止上一次会话(可能是 meow 的 TCP)遗留的协议状态泄漏进本次启动。
fn apply_api_transport(transport: &SidecarApiTransport) -> Result<()> {
    let mihomo = handle::Handle::app_handle().mihomo();
    match transport {
        SidecarApiTransport::IpcSocket { path } => {
            mihomo.update_protocol(tauri_plugin_mihomo::models::Protocol::LocalSocket)?;
            mihomo.update_socket_path(path.to_owned())?;
        }
        SidecarApiTransport::Tcp { host, port, secret } => {
            mihomo.update_protocol(tauri_plugin_mihomo::models::Protocol::Http)?;
            mihomo.update_external_host(Some(host));
            mihomo.update_external_port(Some(*port));
            mihomo.update_secret(secret.as_deref());
        }
    }
    Ok(())
}

#[cfg(test)]
mod sidecar_transport_tests {
    use super::{SidecarApiTransport, sidecar_api_transport};

    /// 工单 02:mihomo 系 sidecar 继续走 IPC socket,不受 meow 引入的 TCP 分流影响。
    #[test]
    fn sidecar_api_transport_routes_mihomo_through_ipc() {
        for core in ["verge-mihomo", "verge-mihomo-alpha"] {
            assert_eq!(
                sidecar_api_transport(core, "/ipc.sock", "127.0.0.1:9097", Some("s")).expect("mihomo transport"),
                SidecarApiTransport::IpcSocket {
                    path: "/ipc.sock".into()
                }
            );
        }
    }

    /// meow 只有 TCP external-controller:地址取自 verge 的 external-controller 设置,
    /// 密钥随行;空密钥按未配置处理,不能把空字符串当鉴权头发出去。
    #[test]
    fn meow_sidecar_runs_on_tcp_and_carries_the_secret() {
        assert_eq!(
            sidecar_api_transport("verge-meow", "/ipc.sock", "127.0.0.1:9097", Some("secret-1"))
                .expect("meow transport"),
            SidecarApiTransport::Tcp {
                host: "127.0.0.1".into(),
                port: 9097,
                secret: Some("secret-1".into()),
            }
        );
        assert!(
            matches!(
                sidecar_api_transport("verge-meow", "/ipc.sock", "127.0.0.1:9097", Some("")),
                Ok(SidecarApiTransport::Tcp { secret: None, .. })
            ),
            "empty secret must degrade to unauthenticated, not an empty bearer token"
        );
    }

    /// meow 没有 IPC 后备:地址坏掉就该在切换前失败并走回滚,而不是起一个摸不到 API 的内核。
    #[test]
    fn meow_sidecar_without_a_usable_controller_fails_fast() {
        for addr in ["", "not-an-addr"] {
            assert!(
                sidecar_api_transport("verge-meow", "/ipc.sock", addr, None).is_err(),
                "controller address {addr:?} must be rejected"
            );
        }
    }
}

#[cfg(target_os = "windows")]
async fn retry_service_start<Start, StartFuture>(
    attempts: usize,
    retry_delay: std::time::Duration,
    mut start: Start,
) -> Result<()>
where
    Start: FnMut() -> StartFuture,
    StartFuture: std::future::Future<Output = Result<()>>,
{
    let mut last_error = None;
    for attempt in 0..attempts {
        match start().await {
            Ok(()) => return Ok(()),
            Err(error) => {
                logging!(
                    warn,
                    Type::Core,
                    "service start attempt {}/{} failed: {error:#}",
                    attempt + 1,
                    attempts
                );
                if error
                    .downcast_ref::<service::ServiceStartRefusal>()
                    .is_some_and(|refusal| service::StageRequest::is_about_the_bundle(refusal.code))
                {
                    return Err(error);
                }
                last_error = Some(error);
                if attempt + 1 < attempts {
                    tokio::time::sleep(retry_delay).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("service start failed")))
}

impl CoreManager {
    /// Restores profile selections before callers enable the system proxy.
    /// The bounded first pass continues in the background, and later calls supersede earlier ones.
    async fn restore_selected_nodes(&self) {
        // 测速 GLOBAL 恢复日志与分组选择无关,不受 pending commit 短路影响;
        // 挂在此处可覆盖 sidecar/service 全部内核启动路径。
        crate::feat::recover_pending_restore().await;
        if PROFILE_SELECTIONS_PENDING_COMMIT
            .try_with(|pending| *pending)
            .unwrap_or(false)
        {
            return;
        }
        crate::config::profiles::restore_selected_nodes().await;
    }
}

const SIDECAR_READINESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
const SIDECAR_READINESS_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(400);

async fn poll_sidecar_readiness<F, Fut>(
    max_attempts: usize,
    retry_delay: std::time::Duration,
    mut probe: F,
) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut last_error = None;
    for attempt in 0..max_attempts {
        match probe().await {
            Ok(()) => return Ok(()),
            Err(error) => {
                logging!(
                    debug,
                    Type::Core,
                    "sidecar readiness probe {}/{} failed: {error:#}",
                    attempt + 1,
                    max_attempts
                );
                last_error = Some(error);
            }
        }
        if attempt + 1 < max_attempts {
            tokio::time::sleep(retry_delay).await;
        }
    }
    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("sidecar readiness was configured with no attempts"))
        .context("Mihomo API did not become ready"))
}

fn should_clear_terminated_sidecar(running_mode: &RunningMode, current_pid: Option<u32>, terminated_pid: u32) -> bool {
    matches!(running_mode, RunningMode::Sidecar) && current_pid == Some(terminated_pid)
}

#[cfg(target_os = "windows")]
use {
    std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
    windows_sys::Win32::{
        Foundation::HANDLE,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
            },
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        },
    },
};

impl CoreManager {
    pub async fn get_clash_logs(&self) -> Result<Vec<String>> {
        match *self.get_running_mode() {
            RunningMode::Service => service::get_clash_logs_by_service().await,
            RunningMode::Sidecar => Ok(CLASH_LOGGER.get_logs()),
            RunningMode::NotRunning => Ok(Vec::new()),
        }
    }

    #[tracing::instrument(skip_all, level = "info", fields(pid = tracing::field::Empty))]
    pub(super) async fn start_core_by_sidecar(&self) -> Result<()> {
        self.wait_for_sidecar_exit().await?;
        let execution = clash_verge_service_ipc::execution::reserve_sidecar()
            .await
            .inspect_err(service::record_residual_service)?;
        self.core_stopped();

        let sidecar_ipc = dirs::sidecar_ipc_path()?;
        let clash_core = Config::verge().await.latest_arc().get_valid_clash_core();
        let clash_info = Config::clash().await.data_arc().get_client_info();
        let transport = sidecar_api_transport(
            clash_core.as_str(),
            dirs::path_to_str(&sidecar_ipc)?,
            &clash_info.server,
            clash_info.secret.as_deref(),
        )?;
        apply_api_transport(&transport)?;
        #[cfg(target_os = "windows")]
        let config_file = if crate::core::runstate::RUN_STATE.state().is_admin {
            Config::generate_file().await?
        } else {
            // Reconciliation persists the preference later; the first spawn must already have TUN off.
            let yaml = sidecar_config_without_tun(&Config::runtime_config_yaml().await?)?;
            Config::write_runtime_file(&yaml).await?
        };
        #[cfg(not(target_os = "windows"))]
        let config_file = Config::generate_file().await?;
        let app_handle = handle::Handle::app_handle();
        let config_dir = dirs::app_home_dir()?;
        #[cfg(unix)]
        discard_unwritable_core_cache(&config_dir);

        #[cfg(unix)]
        let previous_mask = unsafe { tauri_plugin_clash_verge_sysinfo::libc::umask(0o077) };
        let command = app_handle
            .shell()
            .sidecar(clash_core.as_str())
            .map_err(|error| anyhow::anyhow!("failed to build sidecar command for core {clash_core:?}: {error:#}"))?;
        let mut args: Vec<std::string::String> = [
            "-d".to_owned(),
            dirs::path_to_str(&config_dir)?.to_owned(),
            "-f".to_owned(),
            dirs::path_to_str(&config_file)?.to_owned(),
        ]
        .into_iter()
        .collect();
        match &transport {
            SidecarApiTransport::IpcSocket { path } => {
                args.push(
                    if cfg!(windows) {
                        "-ext-ctl-pipe"
                    } else {
                        "-ext-ctl-unix"
                    }
                    .to_owned(),
                );
                args.push(path.clone());
            }
            // meow 不支持 IPC external-controller,用 CLI override 强制 TCP 监听,
            // 不受运行时配置里 external-controller 被禁用(置空)的影响。
            SidecarApiTransport::Tcp { host, port, secret } => {
                args.push("--ext-ctl".to_owned());
                args.push(format!("{host}:{port}"));
                if let Some(secret) = secret {
                    args.push("--secret".to_owned());
                    args.push(secret.clone());
                }
            }
        }
        let command = command.args(args);
        #[cfg(windows)]
        let command = command.env(
            "LISTEN_NAMEDPIPE_SDDL",
            crate::core::owner_identity::current_user_pipe_sddl()?,
        );
        let (mut rx, child) = command.spawn().map_err(|error| {
            anyhow::anyhow!(
                "failed to start sidecar core {clash_core:?} with config {} and data directory {}: {error:#}",
                config_file.display(),
                config_dir.display()
            )
        })?;
        let (terminated, termination) = tokio::sync::oneshot::channel();
        *self.sidecar_exit.lock().await = Some(execution.release_after_exit(child.pid(), termination));
        #[cfg(target_os = "windows")]
        let job = {
            match create_and_assign_sidecar_job(child.pid()) {
                Ok(job) => job,
                Err(job_error) => {
                    let pid = child.pid();

                    let error = match child.kill() {
                        Ok(()) => job_error,
                        Err(kill_error) => anyhow::anyhow!(
                            "failed to configure Job Object for sidecar PID {pid}: \
                            {job_error:#}; failed to terminate child: {kill_error:#}"
                        ),
                    };

                    logging!(error, Type::Core, "Failed to start sidecar: {error:#}");
                    return Err(error);
                }
            }
        };

        #[cfg(unix)]
        unsafe {
            tauri_plugin_clash_verge_sysinfo::libc::umask(previous_mask)
        };

        let pid = child.pid();
        tracing::Span::current().record("pid", pid);

        // The sidecar has to be drained from the moment it starts. tauri-plugin-shell buffers a
        // single event, so an unread channel stalls its reader threads and the core blocks on a
        // full stdout pipe before it ever creates the API socket.
        let core_readiness_generation = self.mark_core_ready();
        AsyncHandler::spawn(move || async move {
            while let Some(event) = rx.recv().await {
                match event {
                    tauri_plugin_shell::process::CommandEvent::Stdout(line)
                    | tauri_plugin_shell::process::CommandEvent::Stderr(line) => {
                        let message = String::from_utf8_lossy(&line).into_owned();
                        Logger::global().writer_sidecar_log(Level::Error, &message);
                        CLASH_LOGGER.append_log(message);
                    }
                    tauri_plugin_shell::process::CommandEvent::Terminated(term) => {
                        let _ = terminated.send(());
                        let manager = Self::global();
                        let _ = manager.invalidate_core_readiness_if(core_readiness_generation);
                        let message = if let Some(code) = term.code {
                            format!("Process terminated with code: {}", code)
                        } else if let Some(signal) = term.signal {
                            format!("Process terminated by signal: {}", signal)
                        } else {
                            String::from("Process terminated")
                        };
                        Logger::global().writer_sidecar_log(Level::Info, &message);
                        CLASH_LOGGER.clear_logs();
                        manager.clear_terminated_sidecar(pid).await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        let readiness = poll_sidecar_readiness(SIDECAR_READINESS_ATTEMPTS, SIDECAR_READINESS_INTERVAL, || async {
            tokio::time::timeout(SIDECAR_READINESS_PROBE_TIMEOUT, async {
                handle::Handle::mihomo().get_version().await
            })
            .await
            .context("Mihomo readiness probe timed out")??;
            Ok(())
        })
        .await;
        if let Err(readiness_error) = readiness {
            proxy_control::stop_guard().await;
            self.core_stopped();
            return match child.kill() {
                Ok(()) => Err(readiness_error),
                Err(kill_error) => Err(anyhow::anyhow!(
                    "{readiness_error:#}; failed to terminate unready sidecar PID {pid}: {kill_error:#}"
                )),
            };
        }

        #[cfg(target_os = "windows")]
        self.set_job_handle(Some(job));
        self.set_running_child_sidecar(child);
        self.core_started(RunningMode::Sidecar);
        self.restore_selected_nodes().await;

        Ok(())
    }

    /// Terminates the sidecar after its caller has successfully cleared the
    /// system proxy.
    pub(super) async fn stop_core_by_sidecar_unprepared(&self) -> Result<()> {
        if let Some(child) = self.take_child_sidecar() {
            let pid = child.pid();

            #[cfg(target_os = "windows")]
            {
                // Setting the job handle to None clears the stored handle and
                // closes the previous Windows job handle in `set_job_handle`.
                self.set_job_handle(None);
                let _ = pid;
            }

            if let Err(error) = child.kill() {
                logging!(warn, Type::Core, "failed to terminate sidecar PID {pid}: {error:#}");
            }
        }
        let result = self.wait_for_sidecar_exit().await;
        self.core_stopped();
        result
    }

    async fn wait_for_sidecar_exit(&self) -> Result<()> {
        let mut exit = self.sidecar_exit.lock().await;
        if let Some(task) = exit.as_mut() {
            tokio::time::timeout(std::time::Duration::from_secs(5), task)
                .await
                .context("Sidecar has not exited; core handoff is blocked")?
                .context("Sidecar exit monitoring failed")?;
            exit.take();
        }
        drop(exit);
        Ok(())
    }

    pub(super) async fn start_core_by_service(&self) -> Result<()> {
        self.core_starting();
        let service_ipc = dirs::ipc_path()?;
        let config_file = Config::generate_file().await?;
        // 服务模式恒走 IPC socket;显式复位,防止 meow sidecar 会话遗留的 Http 协议泄漏进来。
        handle::Handle::app_handle()
            .mihomo()
            .update_protocol(tauri_plugin_mihomo::models::Protocol::LocalSocket)?;
        handle::Handle::app_handle()
            .mihomo()
            .update_socket_path(dirs::path_to_str(&service_ipc)?.to_owned())?;

        self.start_core_by_service_with_config(&config_file).await
    }

    #[tracing::instrument(skip_all, level = "info", fields(config_file = %config_file.display()))]
    pub(super) async fn start_core_by_service_with_config(&self, config_file: &Path) -> Result<()> {
        // 交接时等待 sidecar 释放 ext-controller 通道。
        #[cfg(target_os = "windows")]
        {
            use crate::constants::timing;
            retry_service_start(timing::SERVICE_START_RETRIES, timing::SERVICE_START_RETRY_DELAY, || {
                service::run_core_by_service(config_file)
            })
            .await?;
            self.mark_core_ready();
            self.core_started(RunningMode::Service);
            self.restore_selected_nodes().await;
            service::request_runtime_provider_sync(timing::RUNTIME_PROVIDER_SYNC_DELAY);
            Ok(())
        }

        #[cfg(not(target_os = "windows"))]
        {
            service::run_core_by_service(config_file).await?;
            self.mark_core_ready();
            self.core_started(RunningMode::Service);
            self.restore_selected_nodes().await;
            service::request_runtime_provider_sync(crate::constants::timing::RUNTIME_PROVIDER_SYNC_DELAY);
            Ok(())
        }
    }

    pub(super) async fn stop_core_by_service(&self) -> Result<()> {
        service::stop_core_by_service().await?;
        self.core_stopped();
        Ok(())
    }

    async fn clear_terminated_sidecar(&self, terminated_pid: u32) {
        let _life = self.lifecycle_lock.lock().await;
        if !should_clear_terminated_sidecar(&self.get_running_mode(), self.get_running_sidecar_pid(), terminated_pid) {
            return;
        }

        let _ = self.take_child_sidecar();
        #[cfg(target_os = "windows")]
        self.set_job_handle(None);
        proxy_control::stop_guard().await;
        self.core_stopped();
    }
}

/// Drops a `cache.db` the current user cannot write before handing the directory to the core.
///
/// mihomo keeps `profile.store-selected` in `cache.db` inside its data directory. Service builds
/// before the runtime staging rework ran the core as root against this same directory without a
/// umask, leaving the file as `root:staff 0644`: still readable, so the core loads stale
/// selections, but never writable again, so it silently stops recording new ones. Nothing
/// repairs it either, because the service-side cleanup only runs inside the service. Removing it
/// lets the core recreate the cache under the current user; the fake-ip leases and frozen
/// selections that go with it could not be updated anyway.
#[cfg(unix)]
fn discard_unwritable_core_cache(config_dir: &Path) {
    let cache = config_dir.join("cache.db");
    // Appending neither creates nor truncates, so this only asks whether a write would be allowed.
    match std::fs::OpenOptions::new().append(true).open(&cache) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            // Unlinking is governed by the directory, which the current user owns.
            match std::fs::remove_file(&cache) {
                Ok(()) => logging!(
                    info,
                    Type::Core,
                    "Discarded a core cache the current user cannot write: {}",
                    cache.display()
                ),
                Err(error) => logging!(
                    warn,
                    Type::Core,
                    "Failed to discard the unwritable core cache {}: {error}",
                    cache.display()
                ),
            }
        }
        Err(error) => logging!(
            warn,
            Type::Core,
            "Failed to probe the core cache {}: {error}",
            cache.display()
        ),
    }
}

#[cfg(all(test, unix))]
mod core_cache_tests {
    use super::discard_unwritable_core_cache;
    use std::os::unix::fs::PermissionsExt as _;

    fn scratch(name: &str) -> anyhow::Result<std::path::PathBuf> {
        let root = std::env::temp_dir().join(format!("clash-verge-core-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        Ok(root)
    }

    #[test]
    fn an_unwritable_cache_is_discarded() -> anyhow::Result<()> {
        // root ignores the permission bits, so the probe cannot fail there.
        if unsafe { tauri_plugin_clash_verge_sysinfo::libc::geteuid() } == 0 {
            return Ok(());
        }
        let root = scratch("unwritable")?;
        let cache = root.join("cache.db");
        std::fs::write(&cache, b"stale")?;
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o444))?;

        discard_unwritable_core_cache(&root);

        assert!(!cache.exists(), "an unwritable cache must not be handed to the core");
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn a_writable_cache_is_kept() -> anyhow::Result<()> {
        let root = scratch("writable")?;
        let cache = root.join("cache.db");
        std::fs::write(&cache, b"live")?;

        discard_unwritable_core_cache(&root);

        assert_eq!(
            std::fs::read(&cache)?,
            b"live",
            "a writable cache carries the stored selections and must survive"
        );
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn a_missing_cache_is_not_an_error() -> anyhow::Result<()> {
        let root = scratch("missing")?;

        discard_unwritable_core_cache(&root);

        assert!(!root.join("cache.db").exists());
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}

#[cfg(test)]
mod readiness_tests {
    use super::{claim_core_readiness_generation, poll_sidecar_readiness, should_clear_terminated_sidecar};
    use crate::core::manager::{CoreManager, RunningMode};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    #[tokio::test]
    async fn sidecar_readiness_poll_is_bounded_and_accepts_a_real_api_response() -> anyhow::Result<()> {
        let attempts = Arc::new(AtomicUsize::new(0));
        let probe_attempts = Arc::clone(&attempts);
        poll_sidecar_readiness(3, Duration::ZERO, move || {
            let attempt = probe_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if attempt == 3 {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("not ready"))
                }
            }
        })
        .await?;
        assert_eq!(attempts.load(Ordering::SeqCst), 3);

        let failed_attempts = Arc::new(AtomicUsize::new(0));
        let probe_attempts = Arc::clone(&failed_attempts);
        assert!(
            poll_sidecar_readiness(3, Duration::ZERO, move || {
                probe_attempts.fetch_add(1, Ordering::SeqCst);
                async { Err(anyhow::anyhow!("still unavailable")) }
            })
            .await
            .is_err()
        );
        assert_eq!(failed_attempts.load(Ordering::SeqCst), 3);
        Ok(())
    }

    #[test]
    fn only_the_current_sidecar_termination_clears_local_state() {
        assert!(should_clear_terminated_sidecar(&RunningMode::Sidecar, Some(42), 42));
        assert!(!should_clear_terminated_sidecar(&RunningMode::Sidecar, Some(43), 42));
        assert!(!should_clear_terminated_sidecar(&RunningMode::Service, Some(42), 42));
        assert!(!should_clear_terminated_sidecar(&RunningMode::NotRunning, None, 42));
    }

    #[test]
    fn core_readiness_generation_can_only_be_claimed_once() {
        let generation = AtomicU64::new(7);

        assert!(claim_core_readiness_generation(&generation, 7));
        assert_eq!(generation.load(Ordering::Acquire), 8);
        assert!(!claim_core_readiness_generation(&generation, 7));
    }

    #[test]
    fn invalidated_core_readiness_cannot_be_recaptured_from_stale_mode() {
        let manager = CoreManager::isolated();
        manager.mark_core_ready();
        manager.core_started(RunningMode::Service);

        manager.invalidate_core_readiness();

        assert_eq!(*manager.get_running_mode(), RunningMode::Service);
        assert_eq!(manager.current_core_readiness_generation(), None);
    }
}

#[cfg(target_os = "windows")]
fn create_and_assign_sidecar_job(child_pid: u32) -> Result<OwnedHandle> {
    unsafe {
        let raw_job: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw_job.is_null() {
            return Err(last_win32_error("CreateJobObjectW failed"));
        }
        let job = OwnedHandle::from_raw_handle(raw_job);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        let set_info_result = SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set_info_result == 0 {
            return Err(last_win32_error("SetInformationJobObject failed"));
        }

        let raw_process_handle = OpenProcess(
            PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_INFORMATION,
            0,
            child_pid,
        );
        if raw_process_handle.is_null() {
            return Err(last_win32_error("OpenProcess failed"));
        }
        let process_handle = OwnedHandle::from_raw_handle(raw_process_handle);

        let assign_result = AssignProcessToJobObject(job.as_raw_handle(), process_handle.as_raw_handle());
        if assign_result == 0 {
            return Err(last_win32_error("AssignProcessToJobObject failed"));
        }

        Ok(job)
    }
}

#[cfg(target_os = "windows")]
fn last_win32_error(operation: &'static str) -> anyhow::Error {
    anyhow::Error::new(std::io::Error::last_os_error()).context(operation)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::create_and_assign_sidecar_job;
    use anyhow::Result;
    use std::{
        process::{Child, Command, Stdio},
        thread::sleep,
        time::{Duration, Instant},
    };

    // Use ping directly as a long-lived process without a cmd.exe intermediary.
    fn spawn_long_lived() -> Result<Child> {
        let child = Command::new("ping")
            .args(["-n", "999", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(child)
    }

    fn wait_until_exited(child: &mut Child, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if child.try_wait()?.is_some() {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn job_kills_child_on_handle_drop() -> Result<()> {
        let mut child = spawn_long_lived()?;

        let job = create_and_assign_sidecar_job(child.id())?;

        assert!(
            child.try_wait()?.is_none(),
            "child should still be running after being assigned to the job"
        );

        drop(job);

        assert!(
            wait_until_exited(&mut child, Duration::from_secs(5))?,
            "child should be terminated after the job handle is dropped"
        );

        Ok(())
    }

    #[test]
    fn returns_err_for_invalid_pid() {
        // Windows PIDs are multiples of four; this one is effectively impossible.
        let result = create_and_assign_sidecar_job(0xFFFF_FFFC);
        assert!(result.is_err(), "expected Err for a non-existent PID");
    }
}
