# 03: 配置管线按内核打补丁

**What to build:** 配置管线末端新增「按当前内核打补丁」的纯函数接缝:输入运行时配置 + 当前内核,输出补丁后 YAML。meow 分支覆盖:TUN 场景强制 `dns.enhanced-mode: fake-ip`、需要剔除/替换的 meow 不支持键。测速通道注入的内核分叉在本票先落接缝(mihomo 分支维持现状,meow 分支由工单 05 填充)。单测覆盖;两个内核共用同一份订阅/覆写产出的总体格局由此成立。

**Blocked by:** 02(需要内核枚举与切换流程先存在)

**Status:** ready-for-agent

- [ ] 补丁纯函数:输入配置 + 当前内核,输出符合各内核期望的 YAML;单测覆盖各分支
- [ ] meow 下开启 TUN 时 DNS 模式强制 fake-ip
- [ ] meow 下 mihomo 专有键的处置有明确清单与对应测试(warn-skip 之外需要动的键)
- [ ] mihomo 路径回归:现有配置产出逐字节不变
- [ ] meow 启动加载补丁后配置,日志无配置错误
- [ ] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
