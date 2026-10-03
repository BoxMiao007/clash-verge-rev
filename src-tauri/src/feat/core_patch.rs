//! 按当前内核给运行时配置打补丁(工单 03)。
//!
//! 订阅/覆写管线单份产出 mihomo 兼容 YAML;meow 激活时在管线末端把
//! meow 不支持的 mihomo 专有键剔除、TUN 场景强制 `dns.enhanced-mode: fake-ip`。
//! mihomo 系内核零改动(逐字节回归由测试守护)。
//!
//! 剔除清单的依据:`.scratch/meow-rs-core/spike-notes.md` 项 e 的实测结论
//! (meow 对未知键静默忽略、对部分上游字段 warn,均不报错),以及 meow
//! v0.21.2 `crates/meow-config/src/raw.rs` 的 RawConfig/RawTun/RawDns/
//! RawListener schema 逐键核对。剔除不是功能抢救(忽略亦无害),而是
//! 消除「键看似存在实则无效」的静默漂移并保持 meow 日志干净。

use serde_yaml_ng::{Mapping, Value};

use crate::config::IVerge;

/// meow 无对应字段、需剔除的顶层 mihomo 专有键。
///
/// 逐键依据(meow v0.21.2 `crates/meow-config/src/raw.rs::RawConfig` 无此字段,
/// 非 strict 反序列化静默丢弃,见 spike 项 e):
/// - `unified-delay`/`tcp-concurrent`/`find-process-mode`/`global-client-fingerprint`/
///   `geodata-mode`:mihomo 行为开关,verge 模板或订阅会带入;
/// - `profile`:整节剔除。meow 无 profile 配置面,分组选择持久化走自身
///   `selector-cache.json`,fake-ip 持久化在 `dns.store-fake-ip`,mihomo 的
///   `profile.store-selected`(默认 Merge 模板自带)在此无意义;
/// - `tunnels`:meow 不支持隧道转发,保留会让人误以为生效;
/// - `external-controller-unix`/`external-controller-pipe`/`external-controller-cors`:
///   mihomo 的 IPC 控制器与 CORS 配置;meow 只有 TCP `external-controller`
///   (sidecar 下由 CLI `--ext-ctl` 覆盖,见 manager/state.rs)。
const MEOW_UNSUPPORTED_TOP_LEVEL_KEYS: &[&str] = &[
    "unified-delay",
    "tcp-concurrent",
    "find-process-mode",
    "global-client-fingerprint",
    "geodata-mode",
    "profile",
    "tunnels",
    "external-controller-unix",
    "external-controller-pipe",
    "external-controller-cors",
];

/// meow RawTun 按「上游字段 warn-and-ignore」桶处理的键(verge TUN 模板会产出前三个,
/// 保留只会刷 WARN 日志,见 spike 项 e 实测 stack/auto-detect-interface)。
const MEOW_UNSUPPORTED_TUN_KEYS: &[&str] = &["stack", "strict-route", "auto-detect-interface"];

/// verge DNS 设置页会写入、meow RawDns 无对应字段的键(spike 项 e 静默忽略档)。
const MEOW_UNSUPPORTED_DNS_KEYS: &[&str] = &["fake-ip-range6", "prefer-h3", "respect-rules"];

/// 按当前内核补丁运行时配置(纯函数):meow 分支就地剔除/改写,mihomo 系零改动。
pub fn patch_config_for_core(config: &mut Mapping, core: &str) {
    if !IVerge::is_meow_core(core) {
        return;
    }
    patch_for_meow(config);
}

fn patch_for_meow(config: &mut Mapping) {
    for key in MEOW_UNSUPPORTED_TOP_LEVEL_KEYS {
        config.remove(*key);
    }

    if let Some(tun) = config.get_mut("tun").and_then(Value::as_mapping_mut) {
        for key in MEOW_UNSUPPORTED_TUN_KEYS {
            tun.remove(*key);
        }
    }
    if let Some(dns) = config.get_mut("dns").and_then(Value::as_mapping_mut) {
        for key in MEOW_UNSUPPORTED_DNS_KEYS {
            dns.remove(*key);
        }
    }

    strip_listener_proxy_fields(config);
    force_fake_ip_dns_for_tun(config);
}

/// meow 的 TUN 走 fake-ip DNS 解析路径(spec「配置管线」节定案):TUN 开启时
/// 把 `dns.enhanced-mode` 强制为 `fake-ip`,拦截订阅/Merge 覆写可能带入的
/// redir-host。无 dns 节时补一个最小声明;TUN 未开启不触碰用户的 DNS 选择。
fn force_fake_ip_dns_for_tun(config: &mut Mapping) {
    let tun_enabled = config
        .get("tun")
        .and_then(Value::as_mapping)
        .and_then(|tun| tun.get("enable"))
        == Some(&Value::Bool(true));
    if !tun_enabled {
        return;
    }

    let dns = config
        .entry(Value::from("dns"))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if !dns.is_mapping() {
        *dns = Value::Mapping(Mapping::new());
    }
    if let Some(dns_mapping) = dns.as_mapping_mut() {
        dns_mapping.insert(Value::from("enhanced-mode"), Value::from("fake-ip"));
    }
}

fn strip_listener_proxy_fields(config: &mut Mapping) {
    let Some(listeners) = config.get_mut("listeners").and_then(Value::as_sequence_mut) else {
        return;
    };
    for item in listeners.iter_mut() {
        if let Some(listener) = item.as_mapping_mut() {
            listener.remove("proxy");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "tests assert by panicking")]
mod tests {
    use super::patch_config_for_core;
    use serde_yaml_ng::{Mapping, Value};

    /// 组一份贴近真实管线产出的运行时配置:含 verge 模板键、订阅常见 mihomo
    /// 专有键、GUI TUN 键、测速 listener 与 profile 节。
    fn representative_runtime_config() -> Mapping {
        let mut tun = Mapping::new();
        tun.insert("enable".into(), true.into());
        tun.insert("stack".into(), "mixed".into());
        tun.insert("auto-route".into(), true.into());
        tun.insert("strict-route".into(), false.into());
        tun.insert("auto-detect-interface".into(), true.into());
        tun.insert("dns-hijack".into(), ["any:53"].into_iter().collect::<Value>());

        let mut dns = Mapping::new();
        dns.insert("enable".into(), true.into());
        dns.insert("enhanced-mode".into(), "fake-ip".into());
        dns.insert("fake-ip-range".into(), "198.18.0.1/16".into());
        dns.insert("fake-ip-range6".into(), "fdfe:dcba:9876::1/64".into());
        dns.insert("prefer-h3".into(), false.into());
        dns.insert("respect-rules".into(), false.into());

        let mut profile = Mapping::new();
        profile.insert("store-selected".into(), true.into());

        let mut listener = Mapping::new();
        listener.insert("name".into(), "verge-speedtest".into());
        listener.insert("type".into(), "mixed".into());
        listener.insert("port".into(), 9666.into());
        listener.insert("listen".into(), "127.0.0.1".into());
        listener.insert("proxy".into(), "GLOBAL".into());

        let mut config = Mapping::new();
        config.insert("mixed-port".into(), 7897.into());
        config.insert("log-level".into(), "info".into());
        config.insert("ipv6".into(), true.into());
        config.insert("unified-delay".into(), true.into());
        config.insert("tcp-concurrent".into(), true.into());
        config.insert("find-process-mode".into(), "strict".into());
        config.insert("global-client-fingerprint".into(), "chrome".into());
        config.insert("geodata-mode".into(), true.into());
        config.insert("tun".into(), tun.into());
        config.insert("dns".into(), dns.into());
        config.insert("profile".into(), profile.into());
        config.insert("listeners".into(), vec![Value::Mapping(listener)].into());
        config
    }

    /// 工单 03 验收:mihomo 路径回归,现有配置产出逐字节不变。
    #[test]
    fn mihomo_cores_leave_the_serialized_output_byte_identical() {
        for core in ["verge-mihomo", "verge-mihomo-alpha"] {
            let mut config = representative_runtime_config();
            let before = serde_yaml_ng::to_string(&config).unwrap();

            patch_config_for_core(&mut config, core);

            let after = serde_yaml_ng::to_string(&config).unwrap();
            assert_eq!(before, after, "{core} 路径不得改动任何字节");
        }
    }

    /// spike 项 e + meow v0.21.2 RawConfig schema 核对出的顶层专有键:meow 下全部剔除。
    #[test]
    fn meow_strips_mihomo_only_top_level_keys() {
        let mut config = representative_runtime_config();
        config.insert("tunnels".into(), Vec::<Value>::new().into());
        config.insert("external-controller-unix".into(), "/tmp/ctrl.sock".into());
        config.insert("external-controller-pipe".into(), r"\\.\pipe\ctrl".into());

        let mut cors = Mapping::new();
        cors.insert(
            "allow-origins".into(),
            ["https://yacd.metacubex.one"].into_iter().collect::<Value>(),
        );
        config.insert("external-controller-cors".into(), cors.into());

        patch_config_for_core(&mut config, "verge-meow");

        for key in [
            "unified-delay",
            "tcp-concurrent",
            "find-process-mode",
            "global-client-fingerprint",
            "geodata-mode",
            "tunnels",
            "profile",
            "external-controller-unix",
            "external-controller-pipe",
            "external-controller-cors",
        ] {
            assert!(
                !config.contains_key(key),
                "meow 下 {key} 必须被剔除(meow schema 无此键,静默忽略只会留下漂移)"
            );
        }
        // meow 自身支持的键不受牵连。
        for key in ["mixed-port", "log-level", "ipv6", "tun", "dns", "listeners"] {
            assert!(config.contains_key(key), "meow 支持的 {key} 不得被误删");
        }
    }

    /// meow RawTun 只认 enable/device/mtu/inet4-address/auto-route/dns-hijack 等;
    /// 上游 warn-and-ignore 字段剔除以保持日志干净,verge 模板产出的三个都覆盖。
    #[test]
    fn meow_strips_warned_tun_keys_but_keeps_the_meaningful_ones() {
        let mut config = representative_runtime_config();

        patch_config_for_core(&mut config, "verge-meow");

        let tun = config.get("tun").and_then(Value::as_mapping).unwrap();
        for key in ["stack", "strict-route", "auto-detect-interface"] {
            assert!(!tun.contains_key(key), "tun.{key} 在 meow 下只产 WARN,剔除");
        }
        for key in ["enable", "auto-route", "dns-hijack"] {
            assert!(tun.contains_key(key), "meow 支持的 tun.{key} 不得被误删");
        }
    }

    /// verge DNS 设置页产出的三个 meow 不支持子键(fake-ip-range6/prefer-h3/
    /// respect-rules):meow RawDns 无对应字段,静默忽略,剔除。
    #[test]
    fn meow_strips_unsupported_dns_keys_but_keeps_the_rest() {
        let mut config = representative_runtime_config();

        patch_config_for_core(&mut config, "verge-meow");

        let dns = config.get("dns").and_then(Value::as_mapping).unwrap();
        for key in ["fake-ip-range6", "prefer-h3", "respect-rules"] {
            assert!(!dns.contains_key(key), "dns.{key} 在 meow 下被静默忽略,剔除");
        }
        for key in ["enable", "enhanced-mode", "fake-ip-range"] {
            assert!(dns.contains_key(key), "meow 支持的 dns.{key} 不得被误删");
        }
    }

    /// listener 级 `proxy:` 绑定在 meow 下静默失效(spike 项 e 实证:带该字段时
    /// GLOBAL 切死代理流量仍直达),必须剔除;其余字段与其他 listener 保留。
    #[test]
    fn meow_strips_the_dead_proxy_binding_from_every_listener() {
        let mut config = representative_runtime_config();
        let mut user_listener = Mapping::new();
        user_listener.insert("name".into(), "user-mixed".into());
        user_listener.insert("type".into(), "mixed".into());
        user_listener.insert("port".into(), 7892.into());
        user_listener.insert("listen".into(), "0.0.0.0".into());
        user_listener.insert("proxy".into(), "GLOBAL".into());
        config
            .get_mut("listeners")
            .and_then(Value::as_sequence_mut)
            .unwrap()
            .push(Value::Mapping(user_listener));

        patch_config_for_core(&mut config, "verge-meow");

        for listener in config.get("listeners").and_then(Value::as_sequence).unwrap().iter() {
            let listener = listener.as_mapping().unwrap();
            assert!(
                !listener.contains_key("proxy"),
                "listener {} 的 proxy: 字段在 meow 下静默失效,必须剔除",
                listener.get("name").and_then(Value::as_str).unwrap()
            );
            assert!(listener.contains_key("port"), "其余字段保留");
        }
        // 剔除 proxy 不影响测速通道的端口读取(工单 05 的 IN-PORT 注入依赖它)。
        assert_eq!(
            crate::feat::speedtest_listener_port(&config),
            Some(9666),
            "剔除 proxy 后测速 listener 端口仍可读"
        );
    }

    /// meow 的 TUN 只支持 fake-ip DNS 路径:订阅/Merge 带来的 redir-host 在
    /// TUN 下必须被强制改写(spec「配置管线」节的定案)。
    #[test]
    fn meow_forces_fake_ip_dns_when_tun_is_enabled() {
        let mut config = representative_runtime_config();
        config
            .get_mut("dns")
            .and_then(Value::as_mapping_mut)
            .unwrap()
            .insert("enhanced-mode".into(), "redir-host".into());

        patch_config_for_core(&mut config, "verge-meow");

        let dns = config.get("dns").and_then(Value::as_mapping).unwrap();
        assert_eq!(
            dns.get("enhanced-mode").and_then(Value::as_str),
            Some("fake-ip"),
            "TUN 开启时 redir-host 必须被强制为 fake-ip"
        );
    }

    #[test]
    fn meow_leaves_dns_mode_alone_when_tun_is_disabled() {
        let mut config = representative_runtime_config();
        config
            .get_mut("tun")
            .and_then(Value::as_mapping_mut)
            .unwrap()
            .insert("enable".into(), false.into());
        config
            .get_mut("dns")
            .and_then(Value::as_mapping_mut)
            .unwrap()
            .insert("enhanced-mode".into(), "redir-host".into());

        patch_config_for_core(&mut config, "verge-meow");

        let dns = config.get("dns").and_then(Value::as_mapping).unwrap();
        assert_eq!(
            dns.get("enhanced-mode").and_then(Value::as_str),
            Some("redir-host"),
            "TUN 关闭时用户选择的 DNS 模式不动"
        );
    }

    #[test]
    fn meow_creates_fake_ip_dns_when_tun_is_enabled_without_a_dns_section() {
        let mut config = representative_runtime_config();
        config.remove("dns");

        patch_config_for_core(&mut config, "verge-meow");

        let dns = config.get("dns").and_then(Value::as_mapping).unwrap();
        assert_eq!(
            dns.get("enhanced-mode").and_then(Value::as_str),
            Some("fake-ip"),
            "TUN 开启且无 dns 节时补一个 fake-ip 声明"
        );
    }

    /// 无 tun 节等同 TUN 关闭,不得凭空改写 DNS。
    #[test]
    fn meow_leaves_dns_mode_alone_without_a_tun_section() {
        let mut config = representative_runtime_config();
        config.remove("tun");
        config
            .get_mut("dns")
            .and_then(Value::as_mapping_mut)
            .unwrap()
            .insert("enhanced-mode".into(), "redir-host".into());

        patch_config_for_core(&mut config, "verge-meow");

        let dns = config.get("dns").and_then(Value::as_mapping).unwrap();
        assert_eq!(dns.get("enhanced-mode").and_then(Value::as_str), Some("redir-host"));
    }

    /// 补丁必须幂等:同一配置重复打补丁,产出不变(端口回退/候选校验等路径会
    /// 对同一份草稿多次序列化)。
    #[test]
    fn meow_patch_is_idempotent() {
        let mut config = representative_runtime_config();
        config
            .get_mut("dns")
            .and_then(Value::as_mapping_mut)
            .unwrap()
            .insert("enhanced-mode".into(), "redir-host".into());

        patch_config_for_core(&mut config, "verge-meow");
        let once = serde_yaml_ng::to_string(&config).unwrap();
        patch_config_for_core(&mut config, "verge-meow");
        let twice = serde_yaml_ng::to_string(&config).unwrap();

        assert_eq!(once, twice, "重复打补丁不得再改动");
    }

    /// 工单 05 接缝占位:meow 分支的测速通道注入点。本票只剔除死字段并保证端口可读,
    /// IN-PORT 规则注入由工单 05 在 patch_for_meow 内实现(mihomo 分支维持现状)。
    #[test]
    fn the_meow_branch_is_where_ticket_05_injects_the_in_port_rule() {
        // 本测试是接缝的占位断言:工单 05 实现注入后,这里应改为断言
        // 「栈顶规则 IN-PORT,<测速端口>,GLOBAL 存在且测速恢复路径可用」。
        let mut config = representative_runtime_config();
        patch_config_for_core(&mut config, "verge-meow");
        assert_eq!(
            crate::feat::speedtest_listener_port(&config),
            Some(9666),
            "工单 05 依赖:补丁后测速 listener 端口仍可读"
        );
    }
}
