# 06: 内核升级 + GEO 更新换源

**What to build:** meow 下的手动维护通道:内核「检查更新/升级」复用现有 staging + 回滚机制,更新源按当前内核分叉——meow 指向 meow-rs 官方 GitHub releases 的 Windows zip;GEO 更新改为 fork 侧下载与 mihomo 同格式的 geoip/geosite 文件、替换后重启内核(不依赖 meow 缺席的内核 API)。用户视角:换到 meow 后照样能追新内核、更新规则库,升级失败可回滚。

**Blocked by:** 02(需要 meow 内核已能被托管运行)

**Status:** done

- [x] meow 下检查更新能发现新版本并下载到 staging
  - E2E 实测(WSL dev,`VERGE_E2E=upgrade` 应用内探针,调用与升级按钮完全相同的 `feat::upgrade_core`):钉位 v0.21.2 → GitHub releases/latest API 解析出 v0.22.0 → 资产 `meow-v0.22.0-x86_64-unknown-linux-musl.tar.gz` 下载 2.8s → staging 解包 + `-v` 版本校验通过后原子发布。日志:latest.log `core upgrade: downloading …` → `E2E>>> scenario=upgrade OK: upgraded=true from=0.21.2 to=v0.22.0`(13:18)
  - 证据链真值:发布后 `target/debug/verge-meow -v` = `Meow Meta 0.22.0`,内核 API `/version` = `{"version":"v0.22.0","meta":true}`,新内核进程重启后正常运行(`/tmp/w7.png` 首页经 7897 代理正常出图)
- [x] 升级成功后内核版本号更新;升级失败自动回滚到旧版可用
  - 成功:见上,设置页版本取自内核 API(工单 02 已接),升级后 API 报 v0.22.0
  - 失败回滚 E2E 实测两次:临时注入钩子(`VERGE_TEST_FAIL_RESTART`,已移除)模拟「新内核启动即崩」→ staging 已发布、重启失败 → **自动 rename 回滚 + 旧内核重启**,日志 `core upgrade: rolled back to "0.21.2"`,回滚后 `verge-meow -v` = 0.21.2、内核进程存活、API 正常应答;错误向上抛给 toast(13:30 一轮)
- [x] meow 下 GEO 更新:文件下载替换、内核重启、GEOIP/GEOSITE 规则按新库生效
  - E2E 实测(`VERGE_E2E=geo`,调用与「更新 GEO」按钮 meow 分支完全相同的 `feat::update_meow_geo`):三份文件(Country.mmdb / GeoLite2-ASN.mmdb / geosite.dat,与 meow 启动自下载同源同名,spike 项 a)逐出口探测下载 → 全部就绪后停内核 → 原子替换 → 重启,任一下载失败即中止且不触碰现有文件;日志 `meow geo update: 3 file(s) replaced and the core restarted`,文件 mtime/大小更新(geosite 4253442→4253533B),无临时文件残留
  - 规则生效实证:E2E 订阅临时加 `GEOSITE,CN,DIRECT`,GEO 更新后经 7897 访问 baidu.com,meow 日志 `match GEOSITE(CN) using DIRECT`(13:27)——新库被内核实际加载匹配
- [x] mihomo 下升级与 GEO 更新行为不变(回归)
  - 升级:切回 verge-mihomo 后同探针实测,`upgraded=false from=v1.19.32 to=v1.19.32`(mihomo version.txt 解析路径不变,已是最新即不下载不重启);mihomo 下载/解包代码路径未改,另新增回归单测 `mihomo_unpack_still_takes_the_single_flat_payload`(unix 裸 gz 布局)
  - GEO:mihomo 路径仍是前端 `updateGeo` → 插件 `POST /configs/geo`(代码未动),实测经 IPC socket POST `/configs/geo` = 204
- [x] bump meow 版本时对照 meow 兼容性文档复核差异面(流程写进 bump 步骤)
  - 流程落在 bump 必经入口:`scripts/prebuild.mjs` 的 MEOW_VERSION 读取处注释(bump 前对照 meow 仓库 `docs/mihomo-api-compatibility.md` 复核两点:/configs/geo、/upgrade*、/restart 仍在刻意缺口;兼容面未收缩),结论记工单
  - 本次复核(main HEAD,参照 mihomo commit cbd11db1):上述缺口全部仍在,兼容面未收缩,本工单假设成立
- [x] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
  - typecheck ✓(tsc 无错);pnpm test 60/60 ✓;cargo check ✓(零告警);cargo test 404/404 ✓(新增 8 个:meow 版本归一/tag 解析/资产映射/包 URL/双内核解包/GEO 源表/端口等待×2)

**实现要点(超出验收项的必要改动,E2E 实测发现):**

- **meow sidecar 启动前等待 ext-ctl 端口可绑定**(`core/manager/state.rs`,带单测):meow(tokio)监听套接字不带 SO_REUSEADDR(meow-api 源码裸 `TcpListener::bind`),旧内核被杀后其 API 长连接在内核侧留约 60s TIME_WAIT,期间重绑同一端口 EADDRINUSE;且 meow v0.22.0 起 API 绑定失败为致命错误(v0.21.2 只告警继续跑)——不处理则 meow 下所有重启路径(升级/GEO/改端口)都会撞死。mihomo 走 IPC 不受影响
- **升级回滚判定去竞态**(`feat/core_upgrade.rs`):重启失败的错误返回与内核退出簿记存在毫秒级竞态,「新内核启动即死」会被误判为「新内核在跑」而跳过回滚,留下无内核运行的最糟状态;失败路径先沉降 500ms 再读运行模式
- **meow 包下载逐出口探测**(`feat/core_upgrade.rs`):api.github.com 与 release-assets 走不同域名,各出口可达性可能不同(实测代理出口被 API 限流、直连可达 API 却达不资产域),复用解析出口会挡死可用路线;改为与 geo_update 一致的逐出口探测

**实测方法备注(WSL dev):**

- WSLg 合成输入失效(xdotool/XTEST 对 xmessage 也无效),无法点击界面按钮;验证用环境变量门控的应用内探针调用与按钮完全相同的 feat 层函数(升级=GEO=各自唯一入口),临时代码已全部移除,仅 React→invoke 的 3 行胶水未经点击验证(typecheck 覆盖)
- WSL 无 systemd → service 证据探测必败 → `StartupDecision::Wait` 静默跳过启动,启动需在服务弹窗点「继续使用 SIDECAR」;dev 数据目录 config.yaml 的 external-controller 改为 127.0.0.1:9098(工单 02 同款,避开宿主官方 verge 占用的 9097,WSL mirrored loopback 共享)
- dev 数据目录(~/.local/share/…dev06)留有本地测试订阅 `profiles/e2e-routing.yaml`(http 节点指向宿主代理 7890 模拟真实用户「Localhost 出口经订阅可达 GitHub」),仅本地测试资产不入库

**遗留风险:**

- meow 升级到 v0.22.0+ 后,内核 API 端口被外部程序长期占用时新内核会退出(v0.22.0 致命绑定失败);回滚会保住旧版可用,但该场景升级会持续失败,需用户自行腾出端口
- Windows 的 TIME_WAIT 若长于 Linux(注册表 TcpTimedWaitDelay 历史 default 240s),meow 重启前的端口等待(上限 75s)可能等不到,启动照常进行并依赖就绪探测/回滚兜底;Windows 实机行为待测试包人工验收
- mihomo 的包下载仍复用版本解析出口(上游既有行为,本次未动),理论上存在与 meow 相同的「跨域名出口差异」失败面,如实际遇到再议
