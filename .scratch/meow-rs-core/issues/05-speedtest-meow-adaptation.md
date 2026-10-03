# 05: 测速适配

**What to build:** meow 下节点下载测速全功能可用,方案按工单 01 的定案执行:默认路径为注入栈顶 `IN-PORT,<专用测速端口>,GLOBAL` 规则替代 mihomo 的 listener `proxy:` 绑定,配合既有 GLOBAL 读写接缝与恢复日志,用户视角行为与 mihomo 下一致(整组/单项测速、排序、自定义 URL/时长、测速不改分组选择、崩溃后恢复)。若 01 定案为回退,则本票交付物改为 meow 下测速入口灰显标注,验收标准随之调整。

**Blocked by:** 01(测速适配方案定案)、03(补丁函数接缝就位)

**Status:** done

- [x] meow 下整组测速与单项测速工作,速度数据正确显示
  - WSL dev 实测(真实 app 管线 + 真实 meow v0.21.2 sidecar):运行时配置 `rules[0] = IN-PORT,9667,GLOBAL`、verge-speedtest listener(9667,127.0.0.1,无 proxy 字段);GLOBAL 切 node-local 后经专用端口下载 200(约 2.7 亿-3.5 亿 B/s 本地回环链路),死节点串行遍历即失败态(0 字节超时)——与前端整组测速逐节点调用 `speedtest_node` 的路径等价(前端无内核耦合、无能力灰显)
- [x] 测速期间用户分组选择不变,正常上网不受影响
  - 用户分组 PROXY-SELECT 预先切到 node-local,整轮测速(含负向对照)后 `PROXY-SELECT.now` 保持 node-local;测速只读写 GLOBAL,主 mixed 口 7897 流量不涉 GLOBAL
- [x] 专用端口注入按内核分叉:mihomo 维持现状(回归:配置产出不变)
  - 单测:mihomo 两核序列化逐字节不变 + 规则栈逐项不变(`mihomo_cores_leave_the_serialized_output_byte_identical`、`mihomo_leaves_the_rules_stack_untouched`);实测:切回 verge-mihomo 后运行时配置规则栈无 IN-PORT、listener 带 `proxy: GLOBAL`,死节点对照失败(经 listener 绑定引流)与 meow 行为对齐
- [x] meow 下测速中崩溃/退出,重启后 GLOBAL 恢复到测速前选择(恢复日志生效)
  - 实测 kill -9 现场复现:GLOBAL=node-dead(被测节点)+ 恢复日志 `{original: node-local, target: node-dead}` 在盘、meow `selector-cache.json` 切换即落盘;重启 app 后日志出现 `下载测速:已按恢复日志还原 GLOBAL 选择: node-local`,API 确认 `GLOBAL.now = node-local`,恢复日志文件已清除
- [x] 按速度排序、自定义测速 URL/时长在 meow 下正常
  - 自定义 URL + 3s 窗口实测 200;排序/URL/时长均为前端对 `speedtest_node` 返回 speedBps 的消费与传参,后端命令参数跨内核同一入口(时限/上限归一在 speedtest.rs,内核无关)
- [x] WSL 实测通过(spike 环境复用);Windows 实测随工单 08 测试包
  - 隔离数据目录(`XDG_DATA_HOME`)下 pnpm dev 实测;环境坑见「环境备注」
- [x] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
  - typecheck 通过;vitest 66/66;cargo check 通过;`cargo test --lib` 421 通过(1 ignored 为真实 mihomo e2e)

**实现要点:**

- 注入函数 `inject_speedtest_inport_rule`(feat/speedtest.rs),由 `patch_for_meow` 在剔除 listener 死 `proxy:` 字段后调用:读测速 listener 端口 → 同端口旧规则去重 → 栈顶插入 `IN-PORT,<端口>,GLOBAL`。幂等(重复打补丁不重复注入),用户自有 IN-PORT 规则不受去重影响
- 边界语义:无测速 listener 时不动配置(无通道即无规则);`rules` 缺失或非列表时移除测速 listener——让测速命令以「通道未就绪」显式失败,而非静默测到默认路由的速度;不凭空发明规则列表
- GLOBAL 读写接缝、恢复日志、守卫时序零改动(工单 03 前已存在的测试资产原样通过);内核分叉只发生在配置注入侧(ADR-0004)
- 接缝占位测试 `the_meow_branch_is_where_ticket_05_injects_the_in_port_rule` 按其注释承诺替换为真实断言组(栈顶注入/端口一致/幂等去重/用户规则保留/无 listener 不注入/无 rules 降级/mihomo 回归)

**环境备注(工单 08 Windows 测试包相关):**

- 本机 WSL `networkingMode=mirrored`,宿主 Windows 的 clash API 占 9097 且镜像进 WSL 回环,与 meow sidecar 默认 `--ext-ctl 127.0.0.1:9097` 撞端口(meow 起得来但 TCP API 绑不上,app 侧就绪探测会打到底层不影响功能但 API 不可用)。规避:应用数据目录 `config.yaml` 的 `external-controller` 设为 `127.0.0.1:19097`(app 把该地址原样传给 meow `--ext-ctl`)。Windows 测试包若宿主常驻 clash 同样可能撞 9097,验收时留意
- mihomo sidecar 的 API 只暴露 IPC(`verge-mihomo.sock`,工单 02 分道),外部验证用 `curl --unix-socket`;meow 只支持 TCP。curl 的 `no_proxy=127.*` 通配不生效,127.0.0.1 请求会经系统代理转发(mirrored 下恰好可达 meow,但 mihomo 的 IPC 不可达)——验证脚本需显式绕开代理或走 unix socket
