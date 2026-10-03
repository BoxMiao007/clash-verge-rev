# 07: 内核能力模型 + UI 灰显

**What to build:** 中心化「内核能力」模型落地:一处描述当前内核支持什么(UDP 连接跟踪、规则命中计数、监听热替换、GEO/升级通道归属等),前端接口分流与 UI 灰显都从该模型取事实,组件内不散落内核特判。用户视角的可见行为:meow 下修改端口/监听类设置自动转为内核重启并生效(meow 不支持监听热替换);连接页 UDP 会话缺失与规则页命中计数灰显并标注「meow 不支持」,是能力差异而非故障;mihomo 下一切 UI 行为不变。

**Blocked by:** 02(需要内核枚举;能力清单以 spike 与 spec 兼容性事实源为据)

**Status:** done

- [x] 能力模型:Rust 侧能力判定 + 前端消费点统一,判定逻辑单测覆盖
  - 判定唯一事实源 `src-tauri/src/core/capability.rs`:`CoreCapabilities::for_core` 覆盖 UDP 连接跟踪 / 规则命中计数 / 监听热替换 / GEO 更新与内核升级通道归属(工单 06 分流的事实依据),差异依据 spike 项 d + meow-rs `docs/mihomo-api-compatibility.md`(UDP 会话、rule hit、PUT /configs 普通监听热替换、/configs/geo、/upgrade* 均为刻意缺口);未知识别按 mihomo 处理(宁可热更新失败,不可把 mihomo 用户当 meow 灰显)
  - 经 `get_core_capabilities` 命令暴露;前端唯一消费点 `src/services/core-capabilities.ts` + `src/hooks/use-core-capabilities.ts`(查询未就绪时按 mihomo 基线,保证 mihomo 用户任何加载瞬间不出现灰显);`use-clash.ts` 内核名显示同步收敛到 `getCoreDisplayName`
  - 单测:Rust 6 个(capability.rs,监听键判定×3、能力清单×2、未知内核回退×1)+ 前端 6 个(core-capabilities.test.ts,基线归一、meow 载荷保留、显示名、i18n 文案全 locale 存在性)
  - 工单 06 在 setting-clash.tsx 预留的 GEO 更新内核特判,按其注释约定收敛到 `geoUpdateChannel`(提交 a2cdddbf)
- [x] meow 下修改端口/监听类设置自动重启内核,新端口生效
  - patch_clash 管线(allow-lan 开关等监听类键):meow 下经能力判定转 `restart_core`,mihomo 维持 `update_config_checked` 热更新。E2E 实测(WSL dev,`VERGE_E2E_07=1` 应用内探针调 `feat::patch_clash`,与设置页同一入口):meow 下 allow-lan patch → 内核重启(verge-meow PID 87367→87555,`ss -ltnp` 实测)+ 内核 API `/configs` allow-lan=true;mode patch → 不重启(PID 不变)且 mode=global 生效(非监听键保持热路径)
  - 端口对话框路径(工单 03 引入的 `save_proxy_ports`,两内核本就重启):meow 下 mixed-port 7897→7893 → 重启(PID 87555→87832)+ 7893 监听 + `curl -x 127.0.0.1:7893` 经代理实测返回数据,恢复 7897 同样闭环
- [x] meow 下连接页 UDP 会话缺失有标注、规则页命中计数灰显,文案说明是内核能力差异
  - 截图(GDK_BACKEND=x11 应用窗口实测):连接页「当前内核 Meow 不支持 UDP 连接跟踪,连接列表仅显示 TCP 会话;这是内核能力差异,而非故障。」(/tmp/e2e07-meow-connections.png);规则页「当前内核 Meow 不支持规则命中计数,命中统计不可用;这是内核能力差异,而非故障。」(/tmp/e2e07-meow-rules.png);文案 i18n zh/en,其余 locale 暂复用英文并有守护测试防裸键名
- [x] mihomo 下所有受影响 UI 行为不变(回归)
  - allow-lan patch → 走既有 `update_config_with_force` 热路径,无 restart_core(日志时间线),allow-lan=true 生效;mixed-port 7893 → save_proxy_ports 既有重启行为(PID 91960→92340)+ 新端口代理实测通;连接页/规则页无任何标注(/tmp/e2e07-mihomo-connections.png、/tmp/e2e07-mihomo-rules.png);caps 返回全量能力 + geoUpdateChannel=kernelApi
- [x] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
  - typecheck ✓;pnpm test 66/66 ✓(9 个测试文件);cargo check ✓;cargo test capability 6/6 ✓;移除探针后复跑全绿

**实测方法备注(WSL dev,与工单 06 同款,临时代码已全部移除):**

- WSLg 合成输入失效 → 环境变量门控应用内探针(`VERGE_E2E_07=1`,marker 文件触发场景,结果落 `/tmp/e2e07/*.result`),调与 UI 完全相同的 feat 层入口(patch_clash / save_proxy_ports / continue_with_sidecar);截图用 GDK_BACKEND=x11 + 前端 `VITE_E2E_PAGE` 跳转探针,验证后整文件移除
- dev 数据目录内核二进制为占位 stub,实测前从主仓库复制 verge-mihomo、从 spike 目录复制 meow v0.21.2(与 package.json `meowCoreVersion` 钉版一致)入工作树 sidecar(不入库)
- 实测后 dev 数据目录 verge.yaml 的 clash_core 恢复为 verge-meow(原状)

**遗留风险:**

- 非 zh/en locale 的标注文案暂为英文占位(zhtw/jp/ko 等未翻译),与「其余先复用英文」的票内约定一致;后续补翻译时只需改各 locale json,守护测试会继续拦截裸键名
- LISTENER_PATCH_KEYS 为显式清单( port/socks-port/mixed-port/redir-port/tproxy-port/listeners/tunnels/allow-lan);meow 上游若新增「监听类但可热替换」的键语义,需对照其兼容性文档更新清单(bump 流程已在 prebuild 注释中约定复核)
- 前端 React→invoke 胶水(get_core_capabilities 调用)经探针在 Rust 侧验证命令输出,页面渲染经截图验证;但「设置页点击 → 内核切换 → 能力随之收敛」的完整点击链未做(输入自动化不可用),与工单 06 同级证据
