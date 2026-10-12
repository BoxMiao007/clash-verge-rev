use crate::config::{Config, IVerge};
use anyhow::Result;
use clash_verge_draft::SharedDraft;
use serde_yaml_ng::Mapping;
use tokio::sync::MutexGuard;

pub async fn patch_clash(patch: &Mapping) -> Result<()> {
    let config_write = Config::try_lock_config_write()?;
    let mut effects = super::effects::clash_effects(patch);
    // 当前内核不支持监听热替换(meow):监听类 patch 转内核重启,
    // 并沿用"重启吞掉热重载"的既有规则(工单 07)。
    if listener_patch_needs_restart(patch).await {
        effects.insert(super::effects::Effect::RestartCore);
        effects.retain(|effect| *effect != super::effects::Effect::ClashConfig);
    }
    super::executor::apply(&config_write, super::executor::Patch::Clash(patch), effects).await
}

/// 当前内核是否不支持监听热替换且 patch 触及监听类键(能力模型判定,工单 07)。
async fn listener_patch_needs_restart(patch: &Mapping) -> bool {
    let core = Config::verge().await.latest_arc().get_valid_clash_core();
    crate::core::capability::CoreCapabilities::for_core(&core).requires_restart_for_listener_patch(patch)
}

/// Apply a patch, then reconcile TUN when its setting changes.
///
/// TUN patches do not always produce a Run State transition, so reconciliation is explicit.
pub async fn patch_verge(patch: &IVerge, not_save_file: bool) -> Result<()> {
    apply_verge_patch(patch, not_save_file).await?;
    if patch.enable_tun_mode.is_some() {
        super::reconcile_tun_availability().await;
    }
    Ok(())
}

/// Apply a patch without post-update reconciliation.
pub(super) async fn apply_verge_patch(patch: &IVerge, not_save_file: bool) -> Result<()> {
    let config_write = Config::try_lock_config_write()?;
    apply_verge_patch_locked(&config_write, patch, not_save_file).await
}

/// Apply a patch with the shared configuration write lock already held.
/// Callers must pass the guard returned by [`Config::lock_config_write`].
pub(super) async fn apply_verge_patch_locked(
    _config_write: &MutexGuard<'_, ()>,
    patch: &IVerge,
    not_save_file: bool,
) -> Result<()> {
    super::executor::apply(
        _config_write,
        super::executor::Patch::Verge {
            patch,
            persist: !not_save_file,
        },
        super::effects::verge_effects(patch),
    )
    .await
}

pub async fn fetch_verge_config() -> Result<SharedDraft<IVerge>> {
    let draft = Config::verge().await;
    let data = draft.data_arc();
    Ok(data)
}
