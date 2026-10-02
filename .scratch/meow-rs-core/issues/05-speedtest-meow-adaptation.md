# 05: 测速适配

**What to build:** meow 下节点下载测速全功能可用,方案按工单 01 的定案执行:默认路径为注入栈顶 `IN-PORT,<专用测速端口>,GLOBAL` 规则替代 mihomo 的 listener `proxy:` 绑定,配合既有 GLOBAL 读写接缝与恢复日志,用户视角行为与 mihomo 下一致(整组/单项测速、排序、自定义 URL/时长、测速不改分组选择、崩溃后恢复)。若 01 定案为回退,则本票交付物改为 meow 下测速入口灰显标注,验收标准随之调整。

**Blocked by:** 01(测速适配方案定案)、03(补丁函数接缝就位)

**Status:** ready-for-agent

- [ ] meow 下整组测速与单项测速工作,速度数据正确显示
- [ ] 测速期间用户分组选择不变,正常上网不受影响
- [ ] 专用端口注入按内核分叉:mihomo 维持现状(回归:配置产出不变)
- [ ] meow 下测速中崩溃/退出,重启后 GLOBAL 恢复到测速前选择(恢复日志生效)
- [ ] 按速度排序、自定义测速 URL/时长在 meow 下正常
- [ ] WSL 实测通过(spike 环境复用);Windows 实测随工单 08 测试包
- [ ] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
