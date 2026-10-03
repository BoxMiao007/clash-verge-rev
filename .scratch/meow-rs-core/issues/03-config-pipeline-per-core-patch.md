# 03: 配置管线按内核打补丁

**What to build:** 配置管线末端新增「按当前内核打补丁」的纯函数接缝:输入运行时配置 + 当前内核,输出补丁后 YAML。meow 分支覆盖:TUN 场景强制 `dns.enhanced-mode: fake-ip`、需要剔除/替换的 meow 不支持键。测速通道注入的内核分叉在本票先落接缝(mihomo 分支维持现状,meow 分支由工单 05 填充)。单测覆盖;两个内核共用同一份订阅/覆写产出的总体格局由此成立。

**Blocked by:** 02(需要内核枚举与切换流程先存在)

**Status:** done

- [x] 补丁纯函数:输入配置 + 当前内核,输出符合各内核期望的 YAML;单测覆盖各分支
  - `src-tauri/src/feat/core_patch.rs::patch_config_for_core(&mut Mapping, &str)` 纯函数,mihomo 系就地零改动;挂接点 `config.rs::generate_with_profiles`(紧跟 `inject_speedtest_listener`,管线末端唯一漏斗,`runtime_config_yaml` 的全部消费方——listener/port/sidecar 启动——均覆盖)
  - 11 条单测:顶层/TUN/DNS 专有键剔除、listener `proxy:` 剥离、TUN fake-ip 强制与不触碰分支、无 dns/tun 节边界、幂等、工单 05 接缝占位
- [x] meow 下开启 TUN 时 DNS 模式强制 fake-ip
  - 单测:开启时 redir-host 改写为 fake-ip、TUN 关/无 tun 节不动用户选择、无 dns 节补最小声明
  - 实测:profile 的 `enhanced-mode` 标记为 redir-host,`enable_tun_mode: true` 启动 dev,补丁后 `clash-verge.yaml` 为 `tun.enable: true` + `dns.enhanced-mode: fake-ip`
- [x] meow 下 mihomo 专有键的处置有明确清单与对应测试(warn-skip 之外需要动的键)
  - 清单见 `core_patch.rs` 常量注释(依据 spike 项 e + meow v0.21.2 raw.rs schema 逐键核对):顶层 `unified-delay/tcp-concurrent/find-process-mode/global-client-fingerprint/geodata-mode/profile/tunnels/external-controller-unix/external-controller-pipe/external-controller-cors`,TUN `stack/strict-route/auto-detect-interface`(warn 档),DNS `fake-ip-range6/prefer-h3/respect-rules`,listener 级 `proxy:` 字段(静默失效,必须剔除);保留 `external-controller`/`secret`(meow TCP API 所需,sidecar 以 `--ext-ctl/--secret` 同值覆盖,与工单 02 机制一致)
  - 实测:补丁后 `clash-verge.yaml` 无任何清单键,listener 无 `proxy:` 而其余字段完整
- [x] mihomo 路径回归:现有配置产出逐字节不变
  - 守护断言 `mihomo_cores_leave_the_serialized_output_byte_identical`:verge-mihomo 与 verge-mihomo-alpha 补丁前后序列化逐字节相等;另做变异验证(临时禁用剔除调用,对应测试转红)确认测试非同义反复
- [x] meow 启动加载补丁后配置,日志无配置错误
  - WSL dev 两轮实测(TUN 关/开):sidecar 日志均为 `Config loaded: mode=rule, proxies=5, rules=1` → `meow-rs is running`,零配置错误、零 unsupported-field WARN;`--ext-ctl 127.0.0.1:9098` API 返回 `{"version":"v0.21.2","meta":true}`;7897 代理 curl 204,meow 日志见连接 `match MATCH() using DIRECT`
- [x] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
  - typecheck 通过;vitest 60/60;cargo check 通过;`cargo test --lib` 394 通过(1 ignored)

**实现要点(超出验收项的必要改动):**

- 工单 05 接缝按计划落位:本票 meow 分支只剔除 listener 死字段 `proxy:` 并保证测速端口可读(`speedtest_listener_port` 断言),IN-PORT 规则注入留给工单 05 在 `patch_for_meow` 内实现;占位测试 `the_meow_branch_is_where_ticket_05_injects_the_in_port_rule` 标注了预期
- 补丁幂等是有意保障:端口回退/候选校验等路径会对同一份草稿多次序列化,幂等测试防止重复打补丁引入漂移
- 剔除不是功能抢救(spike 项 e:所有专有键都被 meow 安全忽略),目的是消除「键看似存在实则无效」的静默漂移并保持 meow 日志干净

**环境备注:** 本机 dev 首次启动曾因 `~/.cargo/registry` 的 crates.io sparse 索引缓存过期(`serial_test` 停在 3.1.0,无 4.0.x)而解析失败,删除该缓存文件后恢复——属本机工具链问题,与仓库无关;网络经系统代理 7890 正常。
