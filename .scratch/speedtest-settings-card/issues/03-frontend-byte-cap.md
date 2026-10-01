# 03: 流量上限前端接入

**What to build:** 「速度测试」卡片新增「测速流量上限」条目(MB 计,0 或留空 = 不限,1–1024 MB 合法),配置持久化;单项测速与整组测速一致透传上限;设一个很小的上限(如 1 MB)整组测速时可见快节点提前截断、结果正常显示。随合入更新 AGENTS.md 功能清单中「节点下载测速」的关键文件/入口描述。

**Blocked by:** 01, 02

**Status:** resolved

- [x] 配置键 `default_speedtest_max_mb`(MB 单位)进入 verge 配置结构与类型声明,patch 通道打通
- [x] 解析/夹取纯函数与既有 URL、时长解析同处速度工具模块:0/未配置/非法(负数、非整数、越界)一律不限;1–1024 MB 合法;由 MB 换算字节传后端
- [x] 直测用例覆盖解析/夹取边界(先例:既有 resolve/clamp 用例)
- [x] 卡片条目即时保存;输入即时夹取;tooltip 说明「提前达到上限会提前结束」「0 为不限」
- [x] SpeedManager 调用链单项与整组一致透传,参数带默认值、既有测试向后兼容
- [x] 条目文案覆盖全部语言文件,界面措辞用「测速流量上限」
- [x] AGENTS.md 功能清单「节点下载测速」行入口描述更新
- [ ] `pnpm typecheck && pnpm test` 通过;应用内人工验证:设 1 MB 上限整组测速,快节点提前截断且结果正常

## Answer

实现提交 `b1106b15`(分支 feat/speedtest-settings-card-03,经 `3e1bb034` 合入集成分支)。

- `utils/speed.ts`:`resolveSpeedtestMaxMb`(输入即时归一,0/负数/非整数归 0 不限、越界夹 1024)与 `resolveSpeedtestMaxBytes`(MB→字节,非法一律不限,语义对齐后端 normalize_max_bytes),4 个边界直测用例覆盖 7 类边界情形;`services/speed.ts` 单项/整组可选 `maxBytes` 透传,3 个契约用例;两处消费方带上限。
- `verge.rs` `Option<u16>` + patch 通道、`global.d.ts` 同步;卡片新条目 + 13 语言文案;i18n 生成物重生成;AGENTS.md 功能清单行更新。
- 验证:typecheck、vitest 60 用例、i18n:check、cargo check 全绿。评审修复 `dcc25a1c` 共享两函数的非法性谓词(行为逐字节等价)。
- 遗留:最后一项的自动化部分全绿;应用内人工验证(设 1 MB 上限整组测速、快节点提前截断且结果正常)待维护者执行。
