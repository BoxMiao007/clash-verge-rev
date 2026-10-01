# 02: 后端测速流量上限能力

**What to build:** 单节点测速命令接受可选的字节上限:限时下载循环内已收字节达到上限即提前结束,速度 = 已下载字节 ÷ 实际用时;不传或 0 不限,行为与现状完全一致。截断不引入新的测量状态,测量状态五态不变;GLOBAL 切换/恢复链路不受影响。

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] 命令参数可选、带默认值,既有前端调用不传该参数时行为逐字节等价于现状
- [x] 上限以字节为单位传入;Rust 侧持有限界常量做服务端校验,非法值归一为不限(与测速时长「两处同步界限」的既有约定一致)
- [x] 达到上限按正常「有结果」计速,窗口耗尽与达标截断两种结束路径互不干扰
- [x] 上下限校验为纯函数并用内嵌测试覆盖(先例:速度计算纯函数)
- [x] 截断下载链路不做集成自动化(仓库无 mock HTTP server 先例),人工验证兜底
- [x] `cargo check` 与内嵌测试通过

## Answer

实现提交 `65454d89`(分支 feat/speedtest-settings-card-02,经 `d9416c08` 合入集成分支)。

- `feat/speedtest.rs`:常量 `MB_BYTES`/`MIN_SPEEDTEST_MAX_MB`(1)/`MAX_SPEEDTEST_MAX_MB`(1024),注释沿用「两处同步」约定;归一纯函数 `normalize_max_bytes(Option<f64>) -> Option<u64>`(未传/0/负数/非整数/越界一律 None 不限;f64 承接避免前端非法值反序列化报错);`timed_download` 循环达标即 break,按实际字节计速;`speedtest_node` 可选透传,GLOBAL 链路零改动。
- `cmd/speedtest.rs`:命令可选 `maxBytes`(同 `durationSecs` 驼峰先例)。
- 内嵌测试 2 个(边界表用独立字面量避免与常量互证);cargo test 22 通过、clippy 0 警告。评审修复 `dcc25a1c` 把注释术语锚定到「测速流量上限」。
- 遗留:截断链路应用内实测(设 1 MB 上限整组测速)待维护者人工验收(工单 03 一并覆盖)。
