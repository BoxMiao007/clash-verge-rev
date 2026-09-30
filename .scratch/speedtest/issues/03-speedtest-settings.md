# 03: 测速设置项(测速 URL 与测速时长)

**What to build:** 用户在设置弹窗中可修改测速 URL 与测速时长(秒),默认值 googlechrome.dmg 与 5 秒;仿照现有「默认测试链接 + 默认延迟超时」的成对模式存入 verge 配置(patch 注册、TS 类型同步),设置 UI 放在同一弹窗、紧邻延迟设置。SpeedManager 与后端命令改为读取设置值,保存后对下一次测速立即生效。非法输入(空 URL、超范围时长)有校验或默认值兜底。

**Blocked by:** 02(命令签名与 SpeedManager 已接收参数)

**Status:** resolved

- [x] 设置弹窗新增两个字段,保存后下一次测速即按新值执行(每次触发测速时从 verge 设置解析)
- [x] 未配置时使用默认值;非法输入不产生崩溃或无效请求(resolveSpeedtestUrl / resolveSpeedtestDurationSecs 兜底,时长夹到 1–30 秒)
- [x] 配置结构、patch 注册、TS 类型三处一致
- [x] zh/en 文案就位(zhtw 亦为真实翻译,其余 locale 英文占位由 05 收尾)
- [x] `pnpm typecheck && pnpm test` 与 `cargo check` 通过(47 项测试全绿)

## Answer

实现提交 `4ecfb1cd`(分支 feat/speedtest-03,已快进合入 feat/speedtest)。

- Rust:`config/verge.rs` 新增 `default_speedtest_url: Option<String>`、`default_speedtest_duration: Option<u16>`,紧邻延迟字段并注册 patch 宏。
- TS:`global.d.ts` IVergeConfig 同步;`utils/speed.ts` 新增 `resolveSpeedtestUrl` / `resolveSpeedtestDurationSecs` 纯函数(空白/非整数/超范围回落内置默认);`hooks/use-proxy-speed-state.ts` 触发测速时解析设置传入既有命令。
- UI:`misc-viewer.tsx` 紧邻延迟设置新增「速度测试链接」(placeholder 显示内置默认)与「速度测试时长」(1–30 秒夹值,30 秒上限用于约束整组测速流量,已写入 tooltip)。
- `src/services/speed.ts` 零改动(为 04 合并留零冲突面);新增 5 个解析函数测试。

## Comments

- 2026-09-30:implementer 子代理完成;原计划与 04 并行,因并发限额 04 被拒,改为 03 先合入、04 串行重派。worktree 缺 gitignored 构建输入(sidecar/resources/dist)系 worktree 方案固有现象,已复制解决。
