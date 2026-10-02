# Spike 实证记录:meow-rs 平级第二内核(工单 01)

- 日期:2026-10-02
- 环境:WSL2 (Ubuntu, linux 6.18 WSL2),官方 release 二进制(非本地构建)
- 被测对象:`meow v0.21.2`,`x86_64-unknown-linux-musl`(静态链接),下载自 `gh release download v0.21.2 --repo meow-rs/meow-rs`,tarball sha256 与官方 `.sha256` 校验一致
- 测试资产目录:`/tmp/meow-spike/`(临时,不入库);API 127.0.0.1:9090,secret `meow-spike-secret`;mixed 7891(默认入口)、7892(专用 listener);上游测试靶为本机 python http.server(8000)

> 排查记录:实验中途一度出现「所有规则不匹配、无连接日志」的假象,根因是 curl 的 `--noproxy '*'` 会把显式 `-x` 指定的代理也绕掉,请求根本没进 meow(直连了靶机)。去掉 `--noproxy` 后全部现象收敛一致。下述结论均出自修正后的命令。此坑对后续在 WSL 复测(工单 05)同样适用:**经代理端口的 curl 不得同时加 `--noproxy '*'`**。

---

## 项 a:`-t -d <dir> -f <config>` 校验与启动

**结论:可行。**

证据:

```
$ meow -t -d /tmp/meow-spike/a-data -f a-config.yaml
INFO meow_config: Config loaded: mode=rule, proxies=4, rules=1
INFO meow: Configuration test passed        # exit=0
$ meow -d a-data -f a-config.yaml &
INFO meow_listener::mixed: Mixed listener 'mixed' on 127.0.0.1:7891 (max_connections=256)
INFO meow_api: REST API listening on 127.0.0.1:9090
$ curl -H "Authorization: Bearer meow-spike-secret" http://127.0.0.1:9090/version
{"version":"v0.21.2","meta":true}
$ curl -x http://127.0.0.1:7891 http://127.0.0.1:8000/
hello-from-python
INFO meow_listener::http_proxy: 127.0.0.1:56606 --> 127.0.0.1:8000 match MATCH() using DIRECT
```

- `--help` 确认 `-f/-d/-t` 与 sidecar/service 启动参数完全一致(另有 `--ext-ctl`、`--secret` 等 override,install/uninstall 子命令给服务模式用)。
- `/version` 返回 `{"version":"v0.21.2","meta":true}`,与 mihomo 契约同形,verge 现有版本探测可直接用。
- 首次启动会自动从 GitHub 下载 Country.mmdb / GeoLite2-ASN.mmdb / geosite.dat 到 `-d` 目录(走环境代理成功)。对工单 06 的意义:GEO 文件机制与 mihomo 同源同名,「fork 侧下载替换后重启」的路径成立;安装包也可预置这三份文件跳过在线下载。

## 项 b:IN-PORT 引流 GLOBAL

**结论:可行。** `IN-PORT,<专用端口>,GLOBAL` 能把专用 listener 端口的流量强制引向 GLOBAL 组,且该行为完全由规则驱动。

配置要点:`listeners` 里 mixed listener **没有** `proxy` 字段(meow 确实不支持,见项 e 的忽略证明),规则栈顶放 `IN-PORT,7892,GLOBAL`,其后 `MATCH,DIRECT`。

死代理三段对照(dead-http = 指向 127.0.0.1:19999 无人监听端口的 http 代理):

| 段 | 条件 | 7892 结果 | 日志 |
| --- | --- | --- | --- |
| 1a | IN-PORT 规则存在,GLOBAL=DIRECT | HTTP 200 | `match IN-PORT(7892) using GLOBAL` |
| 1b | IN-PORT 规则存在,GLOBAL=dead-http | HTTP 000(连接失败=进了死代理) | 同上 |
| 2(对照) | **删除 IN-PORT 规则**,GLOBAL=dead-http | HTTP 200 | `match MATCH() using DIRECT` |

`GET /connections` 佐证(活连接):`chains:["GLOBAL"]`, `rule:"IN-PORT"`, `rulePayload:"7892"`。

- 段 1b 证明流量确实被推进 GLOBAL→死代理(连接被拒);段 2 证明这纯属 IN-PORT 规则的功劳,默认路由不经过 GLOBAL——mihomo 下的对照逻辑在 meow 下完全成立。
- 顺带确认:PUT 切换必须带 `Content-Type: application/json`,否则 415(与 mihomo 一致,verge 客户端本来就带)。

## 项 c:GLOBAL 切换与持久化

**结论:可行。** 切换 API 工作正常,选择在切换瞬间落盘、跨进程重启恢复。

```
$ curl -X PUT -H "Content-Type: application/json" -d '{"name":"dead-http"}' .../proxies/GLOBAL   # 204
$ cat <数据目录>/selector-cache.json      # 此刻进程仍在运行,mtime 已更新 => 切换即落盘(崩溃安全)
{ "GLOBAL": "dead-http" }
# 杀进程重启后:
$ curl .../proxies/GLOBAL => now = dead-http
# 再切回 DIRECT,再重启 => now = DIRECT(两轮闭环均恢复)
```

- 持久化文件:`<数据目录>/selector-cache.json`,普通 JSON,键为组名。与 mihomo 的 bbolt `cache.db` 不同文件名、不同格式,但同在 `-d` 目录:双内核即使共用数据目录也不会互相覆盖(文件名不同),选择状态天然隔离(满足 US16)。
- 落盘时机是「切换即写」而非「退出时写」,测速中崩溃后重启能恢复到测速前选择——工单 05 的恢复依赖成立。

## 项 d:四个 WS 端点

**结论:可行。** 四个端点全部按 mihomo 契约推送,`?token=<secret>` 查询参数鉴权可用(websockets 客户端实测,无需 Authorization 头)。

实测形态(`uv run --with websockets` 采集):

```
/traffic      {"up":311320,"down":311320,"upTotal":4830278,"downTotal":5141598}   # mihomo 只有 up/down;up/down 瞬时值在传输期间正常跳动
/logs         {"payload":"127.0.0.1:56340 --> 127.0.0.1:8000 match MATCH() using DIRECT","type":"info"}   # type/payload 与 mihomo 同形
/connections  {"uploadTotal":..,"downloadTotal":..,"memory":..,"connections":[{...}]}
              单个连接含 id/metadata{network,type,sourceIP,destinationIP,sourcePort,destinationPort,host,dnsMode,process,uid,sourceGeoIP,destinationGeoIP,sniffHost,inboundName,inboundPort,specialProxy}/upload/download/start/chains/rule/rulePayload —— mihomo 同形
/memory       {"inuse":9629696,"oslimit":0}   # mihomo 同形
```

- `/logs` 默认 level=info,支持 `?level=debug|info|warning|error|silent`;有流量即推(连接建立时推 match 行)。
- `/connections` 的 `metadata.inboundPort` 恒为 0(mihomo 会报实际入站端口),verge 前端不消费该字段,无影响。
- 已知缺口不变:无 UDP 会话、无规则命中计数(spec User Story 14/15 已按灰显处理)。

## 项 e:mihomo 专有键 warn-skip

**结论:部分可行,总体满足单份配置管线需求。** 非 strict 模式下 `-t` 通过(exit 0)、启动正常、代理不受影响;但行为分两档:

| 键 | 实际行为 |
| --- | --- |
| `tun.stack`、`tun.auto-detect-interface` | 明确 WARN:`field is not supported in meow-rs and will be ignored; remove it to suppress this warning` |
| `unified-delay`、`tcp-concurrent`、`find-process-mode`、`global-client-fingerprint`、`geodata-mode`、`profile.store-selected`、listener 的 `proxy:` 字段 | **静默忽略**,无任何日志 |

- 静默档为 serde 非 strict 反序列化的默认行为(未知键直接丢弃),不是逐键告警。工单 03 的「明确清单」必须靠 fork 侧补丁函数自己维护,不能指望 meow 日志兜底提示。
- listener `proxy: GLOBAL` 被忽略的实证:带上该字段启动后,GLOBAL 切到死代理,7892 流量仍走 `MATCH,DIRECT` 成功(若该字段生效应失败)。→ mihomo 的测速绑定在 meow 下确实不存在,项 b 的 IN-PORT 注入是唯一路径,定案依据闭环。
- 剔除与保留的取舍:保留专有键无功能风险(忽略),但会留下静默漂移空间(键看似存在实则无效);补丁函数按工单 03 计划「meow 激活时剔除不支持的键」仍是更干净的方案,本 spike 不改变该决定。

---

## 测速适配定案

**IN-PORT 方案可行,按原方案走,不回退。**

依据链:项 b 证明 `IN-PORT,<端口>,GLOBAL` 强制引流且不受默认路由干扰(死代理对照);项 c 证明 GLOBAL 选择切换即落盘、跨重启恢复(测速崩溃恢复依赖);项 e 证明 listener `proxy:` 在 meow 下确实无效(IN-PORT 注入是唯一且充分的路由手段);项 a/d 证明 verge 现有 API/WS 消费形态全部兼容。工单 05 按 spec 原方案执行:meow 激活时注入栈顶规则 `IN-PORT,<测速专用端口>,GLOBAL`,专用 listener 保持 `type: mixed` 无 `proxy` 字段,复用既有 GLOBAL 读写接缝与恢复日志。

对工单 03 的输入:

1. 补丁函数需要剔除/改写的键清单(以本 spike 实测为准):无需为「warn-skip 失败」做任何抢救——所有测试键都被安全忽略;剔除是洁癖优化而非功能必需。
2. `tun.stack` / `tun.auto-detect-interface` 若保留会出 WARN(无害),追求日志干净可在 meow 分支剔除。
3. 测速注入接缝在本票落定:meow 分支 = 删 listener 的 `proxy:` 字段(若有)+ 注入栈顶 `IN-PORT,<port>,GLOBAL`。

对工单 05 的输入:

1. 方案不回退;恢复机制前提(切换即落盘)已验证。
2. WSL 复测注意 curl `--noproxy '*'` 陷阱(见文首排查记录)。
