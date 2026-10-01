# 04: 整组测速与按速度排序

**What to build:** 用户点击分组头部的测速按钮(列表视图与卡片视图都有,位于延迟按钮旁),组内全部可交互成员逐个串行测量,每完成一个节点立即刷新其显示,失败节点不影响其余节点继续;整组进行中重复触发的行为与延迟测试保持一致。排序档循环新增「下载速度」:降序、越大越前、无结果的节点排最后;排序选择沿用现有持久化机制,与延迟档、名称档共存。

**Blocked by:** 02(SpeedManager 批量串行调度依赖单项链路;与 03 号工单无依赖,可并行)

**Status:** resolved

- [x] 整组测速逐个更新显示,全程不并发切换、不遗漏失败节点(`checkListSpeed` 严格串行,单项监听即时刷新,排序通知整批收尾一次——与延迟批量同款语义)
- [x] 两个视图均有入口,测量中与完成后状态正确(proxy-head + proxy-group-tools,含链式分组透传)
- [x] 「下载速度」排序档生效并被记住,与现有档位循环切换顺畅(`ProxySortType` 0|1|2|3,持久化沿用 proxy-head-state)
- [x] SpeedManager 批量串行调度与排序比较函数有测试(新增 4 项,全套 51 项通过)
- [x] zh/en 文案就位(`proxies.page.tooltips.speedCheck` / `sortSpeed`)
- [x] `pnpm typecheck && pnpm test` 与 `cargo check` 通过

## Answer

实现提交 `ff2f5c31`(分支 feat/speedtest-04 rebase 后合入 feat/speedtest)。

- SpeedManager 新增 `activeBatches` 与批量串行入口 `checkListSpeed(proxies, group, url, windowSecs)`:批量开始全组置测量中、单项完成即时通知单项监听、失败落失败态不阻塞、分组排序通知抑制到全部批次收尾(与 DelayManager 一致);原 `checkSpeed` 抽为私有 `measureSpeed`,对外语义不变。
- 接线:`proxy-groups.tsx` 新增 `handleSpeedCheckAll`(URL/时长经 03 的解析函数从 verge 设置取,与单项同源);按钮接入 proxy-head / proxy-group-tools,经 proxy-render / proxy-groups-chain 透传。
- 排序:`use-filter-sort.ts` 档位 3 用 `compareBySpeed`,循环 `% 4`。
- 测试:批量串行 3 项(串行时序 manual gate、失败不阻塞、叠批通知一次)+ 排序档 1 组(降序/靠后/平序稳定)。

## Comments

- 2026-09-30:implementer 子代理完成。裁量记录:测速按钮 SpeedRounded、排序档 BoltRounded;首页 current-proxy-card 的独立排序实现不在本工单范围(工单限定沿用 proxy-head-state)。批量期间全组显示测量中直到轮到该节点,与延迟批量行为一致。
