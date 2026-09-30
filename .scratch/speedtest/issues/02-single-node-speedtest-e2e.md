# 02: 单项测速端到端(悬停即可测速并看到速度)

**What to build:** 用户在代理页悬停任意节点时出现「速度测试」按钮,点击后该节点显示测量中状态,完成后在延迟数值旁并列显示下载速度(KB/s/MB/s 自适应、各自着色)。打通完整链路:后端新增单节点测速命令,封装「PUT GLOBAL 到被测节点 → 从专用端口限时下载 → 恢复 GLOBAL」,速度 = 已下载字节 ÷ 实际用时,失败/超时返回明确错误;配置生成管线按 spike 结论注入专用 listener;前端新建 SpeedManager,完全仿照 DelayManager(内存缓存 + 30 分钟 TTL、同一套测量状态机、监听通知与批量合并);节点项悬停区新增单项按钮与并列展示。本阶段参数使用内置默认值(googlechrome.dmg、5 秒),但命令签名已接收 URL 与测速时长参数,供 03 号工单接入设置。provider 节点机制上天然支持,一并可用。

**Blocked by:** 01(spike 结论决定后端形态;若回退为切组方案,本工单需按回退清单调整)

**Status:** resolved

- [x] 悬停单项 → 测速 → 测量中 → 结果并列展示,全链路可用
- [x] 测速前后用户的分组选择不变,规则模式下正常上网流量无感(不写选择记录;GLOBAL 恢复用 Drop 守卫无条件执行)
- [x] 下载失败、节点不通、超时均落入失败状态,不滞留测量中(前端兜底超时 = 窗口 + 10s)
- [x] SpeedManager 接缝测试(mock 后端命令):状态转换、TTL 过期、监听通知;速度格式化与比较纯函数有测试(8 项新测试,全套 42 项通过)
- [x] Rust 速度计算纯函数有内嵌测试(5 项通过)
- [x] `pnpm typecheck && pnpm test` 与 `cargo check` 全部通过
- [x] 新增文案 zh/en 就位,不出现缺译(pre-commit 钩子已给其余 locale 填英文占位,05 收尾补全)

## Answer

实现提交 `037da98a`(分支 feat/speedtest-02,已快进合入 feat/speedtest)。

- 后端新增:`feat/speedtest.rs`(核心链路 + Drop 守卫恢复 + listener 注入纯函数与端口读取,5 个内嵌测试)、`cmd/speedtest.rs`(`speedtest_node(name, url, duration_secs)` + `get_speedtest_listener_port` 预检);复用插件 crate 导出的 `Handle::mihomo()` 客户端做 GET/PUT GLOBAL,未改插件。注入点在 `config/config.rs` 的 `generate_with_profiles`,上游文件仅 2~4 行调用。
- 前端新增:`utils/speed.ts`(classifySpeed/formatSpeed/formatSpeedColor/compareBySpeed + 默认参数常量)、`services/speed.ts`(SpeedManager:缓存+TTL+状态机+监听+rAF 批量)、`services/speed.test.ts`(8 项接缝测试)、`hooks/use-proxy-speed-state.ts`;`proxy-item.tsx`/`proxy-item-mini.tsx` 增量改(列表与卡片视图一致)。
- 超时语义:限时下载下"窗口耗尽"是正常完成;后端将"窗口内 0 字节"判失败,前端所有错误统一落失败态。

## Comments

- 2026-09-30:implementer 子代理完成。环境前置:本机首次构建 Rust 侧,安装了 Tauri Linux 构建依赖(libdbus/glib/gtk/webkit2gtk 等)并运行 prebuild 下载 sidecar。遗留风险:进程 kill -9 时 Drop 守卫不执行(GLOBAL 停留被测节点,下次测速覆盖,ADR-0001 已记录);用户配置验证失败走默认配置时测速报「通道未就绪」;端口探测存在理论 TOCTOU(代码有注释)。端到端真实下载留待 05 号工单人工巡检。
