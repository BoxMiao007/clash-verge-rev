# 06: 内核升级 + GEO 更新换源

**What to build:** meow 下的手动维护通道:内核「检查更新/升级」复用现有 staging + 回滚机制,更新源按当前内核分叉——meow 指向 meow-rs 官方 GitHub releases 的 Windows zip;GEO 更新改为 fork 侧下载与 mihomo 同格式的 geoip/geosite 文件、替换后重启内核(不依赖 meow 缺席的内核 API)。用户视角:换到 meow 后照样能追新内核、更新规则库,升级失败可回滚。

**Blocked by:** 02(需要 meow 内核已能被托管运行)

**Status:** ready-for-agent

- [ ] meow 下检查更新能发现新版本并下载到 staging
- [ ] 升级成功后内核版本号更新;升级失败自动回滚到旧版可用
- [ ] meow 下 GEO 更新:文件下载替换、内核重启、GEOIP/GEOSITE 规则按新库生效
- [ ] mihomo 下升级与 GEO 更新行为不变(回归)
- [ ] bump meow 版本时对照 meow 兼容性文档复核差异面(流程写进 bump 步骤)
- [ ] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
