# 02: 平级切换打通

**What to build:** 设置页内核选择出现 meow-rs 第三项,与 mihomo 平级一键切换:切到 meow 后 sidecar 模式下系统代理可用、能上网;切回 mihomo 无损;切换失败自动回滚并提示。这是第一个纵向切片——内核枚举扩展、meow 二进制接入(dev 下载脚本 + sidecar 声明,版本号钉住)、切换命令校验/回滚、设置页选项、版本号显示一次打通。默认内核仍是 mihomo,升级 fork 不改变用户当前选择。

**Blocked by:** 01(spike 的 CLI 兼容性与 warn-skip 结论决定切换实现细节)

**Status:** done

- [x] 设置页内核弹窗显示 meow-rs 第三项,当前内核高亮正确
  - 截图 `/tmp/t6.png`、`/tmp/t7.png`:弹窗含 `Mihomo(正式版)/ Mihomo Alpha(预览版)/ Meow /verge-meow(轻量版)`,切换前 Mihomo 高亮,Meow 行 chip 为「轻量版」(13 语言 locales 均已加 `variants.meow`)
- [x] 切到 meow:配置校验通过、内核重启、系统代理可用、浏览器经代理上网
  - meow sidecar 进程:`verge-meow -d <数据目录> -f clash-verge.yaml --ext-ctl 127.0.0.1:9098 --secret …`;sidecar 日志含 `REST API listening on 127.0.0.1:9098`、`Mixed listener 'mixed' on 127.0.0.1:7897`、测速专用 listener `verge-speedtest`
  - `GET 127.0.0.1:9098/version`(带 Bearer)= `{"version":"v0.21.2","meta":true}`;经 7897 混合端口访问 `cp.cloudflare.com/generate_204` = 204,首页 IP 查询经代理成功
- [x] 切回 mihomo:订阅与分组选择不丢,代理恢复
  - 进程恢复 `verge-mihomo -d … -f clash-verge.yaml -ext-ctl-unix <socket>`;`config.yaml`(设置)与 `profiles.yaml` md5 前后一致;7897 代理 204。dev 环境无订阅文件,分组选择持久化隔离已由 spike 项 c 覆盖
- [x] 人为制造切换失败(配置校验不过)自动回滚,用户看到明确提示而非断网
  - 移走 meow sidecar 后点击切换:两条红色 toast「无法切换内核 failed to run validation core "verge-meow" … No such file or directory」(截图 `/tmp/e2e-toast.png`);`verge.yaml` 保持 `clash_core: verge-mihomo`(change_core 校验失败回滚,含此前值的持久化恢复),运行中内核未动,代理 204 不断网
- [x] 关于/设置页显示当前 meow 内核版本号
  - 设置页 `Clash 内核` 行显示 `v0.21.2 Meow`(截图 `/tmp/e2e-version.png`);use-clash 版本后缀按当前内核取名(meow 返回的 meta:true 不再写死 Mihomo)
- [x] WSL dev 环境用 Linux 版 meow sidecar 可完整复现上述流程
  - prebuild 新增 `verge-meow` 任务:从 `meow-rs/meow-rs` 官方 release 下载 `meow-v0.21.2-x86_64-unknown-linux-musl.tar.gz`(静态链接,dev 主机可直接跑),落位 `src-tauri/sidecar/verge-meow-<host-triple>`;含「启动即 meow」与「mihomo↔meow 互切」两条路径
- [x] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`(另有 `cargo test --lib` 383 通过)

**实现要点(超出验收项的必要改动):**

- meow 不支持 mihomo 的 IPC external-controller(实测 `-ext-ctl-unix`/`-ext-ctl-pipe` 直接报错退出),sidecar 启动按内核分流:新增 `sidecar_api_transport` 纯函数(单测覆盖)——mihomo 系走 IPC socket,meow 用 `--ext-ctl <地址> --secret <密钥>` 强制 TCP,API 客户端同步切 Http(LocalSocket/Http 双协议由 tauri-plugin-mihomo 原生支持);两个分支都显式设置协议,服务启动路径同样显式复位,避免会话间协议状态泄漏
- `change_core` 校验失败自动回滚:先持久化新值并校验,失败则恢复原值再返回错误(修复原实现「校验失败仍留下新内核持久化」的启动隐患)

**环境备注(WSL mirrored 网络):** 宿主 Windows 上维护者官方 verge 占用 127.0.0.1:9097,WSL mirrored 模式下 loopback 共享导致 dev 侧 meow 绑定 9097 失败。验证时把设置文件 `config.yaml` 的 `external-controller` 改为 127.0.0.1:9098 后全部通过;真实用户环境无此冲突。
