# 0005 - 并行测速采用固定测速槽,单项也走槽并不再切 GLOBAL

下载测速原本只有一条 GLOBAL 专用通道(ADR-0001),整组测速必须串行(一次只能切一个 GLOBAL),慢到不可接受。决定:配置生成时固定注入 `SPEEDTEST_SLOT_COUNT`(8)个隐藏 selector 组作为「测速槽」,并为每个槽注入一条仅绑 127.0.0.1 的 mixed listener;测速时用 `PUT /proxies/<槽组>` 把节点绑到空闲槽,再从该槽端口限时下载。这样 N 个节点可经各自的槽并发测速,mihomo 与 meow 行为一致:

- mihomo:listener `proxy: <槽组>` 直接出站(槽组 `type: select` + `include-all` + `hidden`);
- meow:listener `proxy:` 被忽略(ADR-0004),改由规则栈顶 `IN-PORT,<槽端口>,<槽组>` 引流,槽组名与端口同批注入,避免 meow 对缺失目标静默回落默认路由。

单项测速同样改用测速槽,不再切换 GLOBAL——既统一了路径,也顺带消除了「global 模式用户在测速窗口内流量被波及」的旧取舍;原 GLOBAL 专用通道保留为「槽不可用」时的兜底(端口探测失败、子组成员等),恢复日志机制随之保留。

槽数量在配置生成时固定(meow 不能热替换 listener),因此固定注入 8 个、运行时按设置并发数(1–8)取用,改并发无需重启内核。槽组 `include-all` 只覆盖 provider/内联节点、不含子组,所以并行只覆盖叶子节点;子组成员走 GLOBAL 串行兜底。注入的槽组会出现在 `/proxies` 与 GLOBAL 成员里,前端按 `verge-speedtest-slot-` 前缀统一过滤,不污染界面。

## Considered Options

- 只优化串行(关闭测速窗口、复用连接池):改动最小、读数可比,但每节点仍独占一个窗口,20 节点仍需百秒级,无法满足「默认并行」的诉求,弃用。
- 运行时新增/删除槽 listener:可精确匹配并发数,但 meow 改 listener 必须重启内核、mihomo 整份 `PUT /configs` 会断开进行中的下载,弃用。
- 用子进程/多开内核并行:隔离最彻底,但资源与复杂度不可接受,弃用。
- 槽组直接绑定节点名(不经 selector):provider 节点不在顶层 `proxies` 映射、无法寻址,且无法在运行时改绑,弃用。

## 影响与约束

- 并发读数共享同一出口带宽,会互相压低,与串行结果不可直接横向比较,主要用于相对排序;总流量约为「并发数 × 窗口」。并发数即唯一的并行开关:1 = 逐个测速,默认 4。
- meow 必须保证槽组与槽规则在生成配置里同时存在,否则会静默测到默认路由;`inject_speedtest_slot_inport_rules` 与槽 listener 同批调用,并在无规则列表时移除槽能力、显式失败。
- 槽选择会写入内核的选择持久化(cache.db / selector-cache.json),因槽是专用隐藏组,不影响用户分组选择;不再污染 GLOBAL。
