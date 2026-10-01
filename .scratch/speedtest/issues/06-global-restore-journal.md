# 06: GLOBAL 恢复日志(崩溃残留修复)

**What to build:** 进程被 kill -9 / 断电时 Drop 守卫不执行,GLOBAL 停留在被测节点并经 mihomo cache.db 跨会话残留——global 模式下用户流量走错节点,直到手动切换或下次测速覆盖。切换 GLOBAL 前把「原选择 → 被测节点」写入应用数据目录的恢复日志(先写临时文件刷盘、再同目录原子改名防半写;写入失败放弃本次测速);恢复成功清日志,失败保留;内核启动后(`CoreManager::restore_selected_nodes` 入口,先于分组选择恢复)检查日志:残留仍在则恢复原选择、已被用户覆盖仅清理、恢复失败(原节点失效)放弃并清理、GLOBAL 读取失败(含无当前选择)保留日志待下次重试。

**Blocked by:** 无

**Status:** resolved

- [x] 先写后切:写日志 → 切换 → 恢复成功清日志(切换失败/无需切换不残留)
- [x] 启动恢复四分支:残留恢复 / 已覆盖仅清理 / 节点失效放弃 / 内核不可读保留重试
- [x] 单测 19 项(文件操作、决策纯函数、守卫集成、恢复分支)全绿
- [x] 真实 mihomo 端到端三场景通过(#[ignore],本地验证;sidecar 产物缺失时跳过)
- [x] `pnpm typecheck && pnpm test` 与 `cargo make rust-clippy` 全绿

## Answer

实现提交见 git log(fix/global-restore-journal → dev):

- `feat/speedtest.rs`:恢复日志读写清(临时文件刷盘 + 原子改名防半写)+ `decide_startup_restore` 决策纯函数;`speedtest_via_global` 增先写后切(切换失败即清、无需切换不写);`GlobalSelectionGuard`/`restore_global_selection` 按「恢复成功才清日志」处理;`recover_pending_restore[_with]` 启动恢复入口。
- `core/manager/state.rs`:`restore_selected_nodes` 开头调用 `feat::recover_pending_restore()`(上游文件仅 1 行调用,覆盖 sidecar/service 全部内核启动路径)。
- 端到端验证用仓库 sidecar 的真实 mihomo(TCP external-controller + REST),三场景断言 GLOBAL 终态与日志清理。
- 已知取舍(ADR-0001 修订):恢复失败放弃不重试,瞬时故障(内核未就绪)保留日志下次重试。
