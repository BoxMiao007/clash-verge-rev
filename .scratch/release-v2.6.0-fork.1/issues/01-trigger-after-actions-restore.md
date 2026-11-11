# 01: 等 Actions 恢复后触发 v2.6.0-fork.1 正式版发布

**What to build:** 2026-11-11 上游同步(基线 2.6.0)已完成并推送:

- `dev` = `36477839`(含 127+4 个上游提交、评审整改)
- `sync/2026-11-11` 标签已推 origin
- 发布标签 `v2.6.0-fork.1` 已推 origin(tag 对象 `f0332642`)

发布被 GitHub **账户级 Actions 封停**卡住:tag 推送未触发 run;`gh workflow run` 返回
`HTTP 422: Actions has been disabled for this repository`。已排除仓库配置问题:
仓库为 public、`actions/permissions` 返回 `enabled=true, allowed_actions=all`、
Fork Release 工作流 `state=active`。

同因记录见 `.scratch/release-v2.5.9-fork.1/issues/01-trigger-after-quota-reset.md`
(2026-10-12 确诊:免费额度吃满且账户未绑付款方式 → 封停;曾预期 11-01 月度重置自动解封,
但 11-11 仍 422)。

**恢复后的操作(一步,无需重推 tag):**

```
gh workflow run fork-release.yml --repo BoxMiao007/clash-verge-rev -f tag=v2.6.0-fork.1
```

预计构建约 35 分钟,产物为 Windows x64 NSIS 安装包,挂在 `v2.6.0-fork.1` 的 Release 下,
版本号由 CI 盖为 `2.6.0+fork.1`。

**验收:**

- [ ] Actions 恢复后 dispatch 成功,workflow run 创建
- [ ] Release 含 x64 安装包,版本显示 2.6.0+fork.1
- [ ] 安装包内含 verge-meow.exe + wintun.dll
- [ ] Windows 实机覆盖安装冒烟:双内核切换、测速(含设置搜索可检索测速项)、meow GEO 更新

**Status:** blocked-on-owner(GitHub 账户 Actions 封停;需维护者查 github.com/settings/billing
是否欠款/需绑卡,或联系 GitHub Support 解封)

**注意:**

- Auto Build 仍保持 `disabled_manually`(额度耗尽元凶),勿启用;fork 发布只走
  Fork Release / Win x64 Test Build。
- 恢复后若 dispatch 仍 422,回报 billing 页信息再排查。
