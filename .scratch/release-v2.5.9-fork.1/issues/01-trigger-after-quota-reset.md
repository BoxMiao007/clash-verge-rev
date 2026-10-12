# 01: 等额度重置后触发 v2.5.9-fork.1 正式版发布

**What to build:** 2026-10-12 上游同步(合并提交 6b5d0b711,基线 2.5.9)已完成并推送,
`v2.5.9-fork.1` 标签已在 origin 上。发布被 GitHub 账户级 Actions 悬停卡住:
免费额度耗尽(证据:10 月 9 日 Auto Build 跑到 13 分钟被强制取消,其后所有
workflow 触发请求一律返回 422 "Actions has been disabled for this repository";
仓库级权限 enabled=true、工作流开关正常,故为账户级停用)。

维护者决策(2026-10-12):**等 11 月 1 日免费额度月度重置**,恢复后立即触发发布,
不为此付费。

**恢复后的操作(一步):**

```
gh workflow run fork-release.yml --repo BoxMiao007/clash-verge-rev -f tag=v2.5.9-fork.1
```

tag 已在远端,无需重推;dispatch 走 fork-release.yml 的 workflow_dispatch 通道
(AGENTS.md「重发同一 tag」节)。预计构建约 35 分钟,产物为 Windows x64 NSIS 安装包,
挂在 v2.5.9-fork.1 的 Release 下,版本号被 CI 盖为 `2.5.9+fork.1`。

**验收:**

- [ ] Actions 恢复后 dispatch 成功,workflow run 创建
- [ ] 出包成功:v2.5.9-fork.1 Release 含 x64 安装包,版本显示 2.5.9+fork.1
- [ ] 安装包内含 meow.exe + wintun.dll(第二内核随包,工单 08 的打包接线)
- [ ] 维护者 Windows 实机覆盖安装冒烟:双内核切换、测速、meow GEO 更新

**Status:** ready-for-agent(前置条件:2026-11-01 额度重置;届时用户说「发」即执行)

**根因确诊(2026-10-12,依据 Billing 页截图):** 10 月计费用量 $44.05 =
免费档包含额度(2,000 分钟 + 500MB 存储)被全额吃满(clash-verge-rev $35.62 +
pixel-arena $7.17 + 其他零头)。账户未绑付款方式,GitHub 按规则
"额度用完且无付款方式 → 封停 Actions"。clash-verge-rev 的分钟按私有仓库费率
计了账(构建期间仓库为私有;现已是公开,公开仓库标准 runner 分钟免费),
成本大头是 Auto Build 矩阵里的 macOS job($0.062/分钟,10 倍单价)。
仓库现转公开 + Auto Build 已禁用,fork 发布构建今后不再消耗额度;
11 月 1 日月度重置后自动解封。

**注意:**

- Auto Build(上游残留的每日两次 3 平台构建)已为 disabled_manually——这是额度
  耗尽的元凶,**保持禁用**,勿再启用;fork 发布只走 Fork Release / Win x64 Test Build。
- 若 11 月 1 日后 dispatch 仍 422:让维护者看 github.com/settings/billing 确认
  账单状态(可能是付款问题而非额度),把页面信息回报给 agent 再排查。

## Comments

- 2026-10-12:推送 dev(6b5d0b711)、sync/2026-10-12、v2.5.9-fork.1 均成功;
  尝试 dispatch Fork Release 与 Win x64 Test Build 均遇账户级 422,确认非仓库配置问题。
