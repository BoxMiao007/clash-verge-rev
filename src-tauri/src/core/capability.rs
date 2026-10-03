use serde::Serialize;
use serde_yaml_ng::Mapping;

/// 内核升级/GEO 更新的执行通道归属:内核自带接口,还是由 fork 侧命令托管。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChannel {
    KernelApi,
    ForkSide,
}

/// 内核能力:某内核实现对外支持的操作与特性集合(GLOSSARY「内核能力」)。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreCapabilities {
    pub core: String,
    pub udp_connection_tracking: bool,
    pub rule_hit_counting: bool,
    pub listener_hot_reload: bool,
    pub geo_update_channel: UpdateChannel,
    pub core_upgrade_channel: UpdateChannel,
}

impl CoreCapabilities {
    /// 能力判定唯一事实源。差异依据:
    /// - spike 实测(.scratch/meow-rs-core/spike-notes.md 项 d)与
    ///   meow-rs `docs/mihomo-api-compatibility.md`:meow 不跟踪 UDP 会话、
    ///   /rules 无命中数据、PUT /configs 不做普通监听热替换、无 /configs/geo 与 /upgrade。
    /// - 两内核的升级都由 fork 侧 staging+回滚托管(ADR-0003),升级通道归属一致。
    /// - 未知识别按 mihomo 处理:能力缺失宁可热更新失败,不可把 mihomo 用户当 meow 灰显。
    pub fn for_core(core: &str) -> Self {
        let meow = crate::config::IVerge::is_meow_core(core);
        Self {
            core: core.into(),
            udp_connection_tracking: !meow,
            rule_hit_counting: !meow,
            listener_hot_reload: !meow,
            geo_update_channel: if meow {
                UpdateChannel::ForkSide
            } else {
                UpdateChannel::KernelApi
            },
            core_upgrade_channel: UpdateChannel::ForkSide,
        }
    }

    /// 给定一次 clash 配置 patch,判断当前内核是否必须重启才能生效。
    ///
    /// meow 的 PUT /configs 只重载代理/规则/mode,不做普通监听热替换
    /// (meow-rs 兼容性文档;allow-lan 影响 bind_addr,同属监听类)。
    /// secret/external-controller 的重启是 patch_clash 既有行为,不经此判定。
    pub fn requires_restart_for_listener_patch(&self, patch: &Mapping) -> bool {
        !self.listener_hot_reload && LISTENER_PATCH_KEYS.iter().any(|key| patch.get(*key).is_some())
    }
}

/// 触发监听重建的 clash 配置键;tun 除外——meow 对 `tun:` 差异会自行重启 TUN listener。
const LISTENER_PATCH_KEYS: &[&str] = &[
    "port",
    "socks-port",
    "mixed-port",
    "redir-port",
    "tproxy-port",
    "listeners",
    "tunnels",
    "allow-lan",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml_ng::Mapping;

    fn patch_with(key: &str) -> Mapping {
        let mut patch = Mapping::new();
        patch.insert(key.into(), 1.into());
        patch
    }

    /// 工单 07:meow 不支持监听热替换,端口/监听类 patch 必须转内核重启;
    /// mode/tun 类键不受影响(meow 的 PUT /configs 会自行重载 mode、重启 TUN listener)。
    #[test]
    fn meow_requires_restart_for_listener_patches() {
        let caps = CoreCapabilities::for_core("verge-meow");

        for key in ["port", "mixed-port", "allow-lan", "listeners", "tunnels"] {
            assert!(
                caps.requires_restart_for_listener_patch(&patch_with(key)),
                "meow 应对 {key} 改动要求内核重启"
            );
        }

        for key in ["mode", "log-level", "tun"] {
            assert!(
                !caps.requires_restart_for_listener_patch(&patch_with(key)),
                "meow 下 {key} 改动不应触发内核重启"
            );
        }
    }

    /// 回归守护:mihomo 保持热更新路径,任何键都不因能力模型转重启。
    #[test]
    fn mihomo_keeps_hot_reload_for_all_patches() {
        let caps = CoreCapabilities::for_core("verge-mihomo");

        for key in ["port", "mixed-port", "allow-lan", "listeners", "tunnels"] {
            assert!(!caps.requires_restart_for_listener_patch(&patch_with(key)));
        }
    }

    /// meow 能力下,不含监听键的 patch 不重启;secret/external-controller
    /// 的重启属既有行为,不归能力模型管。
    #[test]
    fn meow_keeps_hot_path_for_non_listener_patches() {
        let caps = CoreCapabilities::for_core("verge-meow");

        assert!(!caps.requires_restart_for_listener_patch(&Mapping::new()));
    }

    /// 工单 07:meow 的能力差异以 spike 结论与兼容性文档为事实源
    /// (.scratch/meow-rs-core/spike-notes.md 项 d;meow-rs docs/mihomo-api-compatibility.md)。
    #[test]
    fn meow_lacks_udp_tracking_rule_hits_and_listener_hot_reload() {
        let caps = CoreCapabilities::for_core("verge-meow");

        assert!(!caps.udp_connection_tracking);
        assert!(!caps.rule_hit_counting);
        assert!(!caps.listener_hot_reload);
        // meow 无 /configs/geo 与 /upgrade,GEO 更新与内核升级由 fork 侧托管(工单 06 分流)。
        assert_eq!(caps.geo_update_channel, UpdateChannel::ForkSide);
        assert_eq!(caps.core_upgrade_channel, UpdateChannel::ForkSide);
    }

    /// mihomo 系维持全量能力;内核升级虽走 fork 侧 staging+回滚(ADR-0003),
    /// 但归属事实与 meow 一致,避免前端对两内核走两条升级界面逻辑。
    #[test]
    fn mihomo_cores_keep_full_capabilities() {
        for core in ["verge-mihomo", "verge-mihomo-alpha"] {
            let caps = CoreCapabilities::for_core(core);

            assert!(caps.udp_connection_tracking);
            assert!(caps.rule_hit_counting);
            assert!(caps.listener_hot_reload);
            assert_eq!(caps.geo_update_channel, UpdateChannel::KernelApi);
            assert_eq!(caps.core_upgrade_channel, UpdateChannel::ForkSide);
        }
    }

    /// 未知识别按 mihomo 处理:能力缺失宁可热更新失败,不可把 mihomo 用户当 meow 灰显。
    #[test]
    fn unknown_core_falls_back_to_mihomo_capabilities() {
        let caps = CoreCapabilities::for_core("verge-something-else");

        assert!(caps.udp_connection_tracking);
        assert!(caps.rule_hit_counting);
        assert!(caps.listener_hot_reload);
    }
}
