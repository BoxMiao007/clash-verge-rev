# 04: 服务模式 + TUN

**What to build:** Clash Verge Service 以托管 mihomo 相同的方式托管 meow.exe(传 `-d` 数据目录与 `-f` 配置路径),服务模式下 Windows TUN 可用(wintun.dll 随安装包分发)。用户视角:服务模式安装/卸载、启停、TUN 开关在 meow 下与 mihomo 体验无差。开发可在 WSL 先行验证 sidecar 路径,最终验收依赖工单 08 产出的 Windows 测试包实测。

**Blocked by:** 02、03(fake-ip 强制是 TUN 的前提);最终 Windows 实测需 08 的测试包(不阻塞开发)

**Status:** ready-for-human

- [ ] 服务模式启动 meow 成功,启停/切换/卸载与 mihomo 行为一致 —— **被外部依赖阻塞**(见下方「阻塞实证」,非本仓库可解);本票已把 app 侧链路全部打通,待服务端支持后翻开关即可
- [ ] 服务模式 + TUN:Windows 实测全部流量走 TUN 且可上网,DNS 解析正常(fake-ip 生效)—— **待阻塞解除 + 测试包实测**(wintun.dll 进服务目录的分发与 staging 本票已接线,见已勾选项)
- [x] sidecar(无服务)模式下 meow 行为不变(回归)
  - `start_core_by_sidecar` 及其传输分流/端口等待/配置管线零改动;`cargo test --lib` 416 通过(含工单 02 的 sidecar 传输回归测试),前端 66 测试通过;sidecar 模式的 meow 启动参数与 TUN/fake-ip 强制(工单 03)不受本票影响
- [ ] 服务日志可查,内核异常退出有明确报错 —— **待阻塞解除后验证**(服务侧日志采集与看门狗报错对内核无差别:meow 启动失败时服务报 `core exited before its IPC endpoint became ready`,app 侧 `get_clash_logs_by_service` 不分内核;真实报错形态需 Windows 实测确认)
- [x] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`(另有 `cargo test --lib` 416 通过,含本票新增 7 项)

## 阻塞实证:钉住的 Clash Verge Service 无法托管 meow(2026-10-03)

服务端二进制来自上游 release(`scripts/service-release.mjs` 按 Cargo 依赖版本 v2.7.5 下载),其启动行为 app 侧无法绕过:

1. **启动参数硬编码 IPC**:服务端 `core_args()` 对任何内核无条件传 `-d <目录> -f <配置> -ext-ctl-pipe <管道>`(Windows);meow 的 clap 把 `-ext-ctl-pipe` 当短标志簇解析,WSL 实测 meow v0.21.2 直接报 `unexpected argument '-e' found` 退出;meow v0.22.0 源码虽有 `--ext-ctl-pipe` 定义但运行时 bail "not yet supported"(源码已核对)。
2. **就绪判定依赖 IPC pipe**:服务把「内核创建归 IPC named pipe 且服务端进程 PID 等于被 spawn 进程」当启动成功(`secure_core_ipc_socket` + `GetNamedPipeServerProcessId`),meow 无 IPC 能力,即使绕过参数问题也过不了就绪判定;失败归类为 `OwnerSwitchFailed`,app 侧不会走 sidecar 回退。
3. 服务端启动路径由上游 clash-verge-service-ipc 仓库实现并整包发版,v2.7.6(当前最新)与 main 分支均无 TCP-only 内核支持(`core_args` 与就绪判定逐版核对)。

**解除路径(维护者决策,任选其一):**

- A. fork clash-verge-service-ipc,给 `CoreConfig` 增加「内核原样参数」形态(TCP-only 内核跳过 IPC 参数、就绪判定改用进程存活),fork CI 发版后本仓库 bump 依赖与 service 下载版本;
- B. 等 meow 上游实现 named-pipe external-controller(其 CLI 已留定义);
- C. 接受「meow + 服务模式 = 显式不可用」现状,关闭工单两条服务验收项(能力模型已按此呈现)。

## 本票已交付(app 侧链路全部就绪)

- **服务 staging 伴随文件**:`core_staging_companions`(`src-tauri/src/core/service.rs`)——meow 进服务目录时随行 wintun.dll(meow 按自身所在目录搜索 TUN 驱动),mihomo 系自嵌驱动无伴随文件;缺失跳过不阻塞 core 本体。`install_service`(服务安装)与 `stage_approved_core`(内核升级移交,工单 06 路径)两个入口都消费该清单
- **安装期 staging**:prebuild 的 `core-hashes.nsh` 新增 `MEOW_SHA256`/`WINTUN_SHA256`;`installer.nsi` 在 mihomo 系 staging 块后追加 verge-meow.exe 与 wintun.dll 的 `--install-core`(带 digest 出证,顺序在 wintun.dll 挪到安装根之后)。服务解析 `core_path` 按**文件名**映射到服务目录的核准副本,verge-meow.exe 不在核准目录时服务拒绝并提示补投
- **服务模式 API 传输按内核分流**:`service_api_transport`(`src-tauri/src/core/manager/state.rs`)与 sidecar 共用同一份分流事实——mihomo 系指向服务托管的 IPC socket,meow 走 TCP external-controller;`start_core_by_service` 改经 `apply_api_transport` 显式设置协议,meow 的服务启动同样等待 ext-ctl 端口可绑定(复用工单 06 等待,覆盖 sidecar→服务交接的 TIME_WAIT 窗口)
- **能力事实收敛**:`CoreCapabilities.service_hosting`(meow=false)进能力模型;首页「运行模式」服务安装入口按能力分流,meow 激活时提示「Meow 内核暂不支持服务模式」(13 语言 `coreNotHostable`),不再发起注定失败的服务安装;服务端支持后把判定翻回 true 即可放通,启动链路无需再改

**已知残余路径(受同一外部阻塞,行为安全):** 服务模式已运行时切换到 meow → 服务启动失败、切换回滚,报错可见;meow sidecar 下开启 TUN 触发的服务迁移对话框仍可发起安装,安装后服务重启内核失败,可继续用 sidecar(既有回退行为)。两者在 Windows 实测前不虚报可用。

**维护者决策(2026-10-03):选 C——接受现状。** meow 定位为轻量系统代理内核(sidecar 模式),TUN 需求切回 mihomo;A/B 两条解除路径留作将来可选工作,届时翻 `service_hosting` 能力开关即可放通,启动链路无需再改。
