# 0003 - meow-rs 作为平级第二内核接入

上游 clash-verge-rev 已拒绝接入 meow-rs(issue #7571,Closed as not planned,无讨论):meow-rs 是 mihomo 的 Rust 重写,官方 Windows x64 二进制 7.8MB、内存约为 mihomo 的 1/3,REST/WS API 与配置 schema 高度兼容但非同源。决定:fork 以**平级第二内核**形态接入——内核枚举新增 meow 取值,设置页与 mihomo/mihomo Alpha 平级一键切换,复用既有「配置校验 → 更新设置 → 重启内核」流程,校验失败自动回滚;默认内核保持 mihomo,升级 fork 不改变用户当前选择。两个内核共用单份订阅/覆写配置产出,在管线末端按当前内核打补丁(剔除 meow 不支持的键、TUN 场景强制 fake-ip、测速通道切换注入方式,见 ADR-0004),不维护两份配置模板。内核间差异不在组件里散落判断,由中心化的**内核能力**模型统一描述:UI 灰显与接口分流都从它取事实;meow 刻意不实现且 fork 能补的(重启、GEO 更新、内核升级)包装为 fork 侧命令分流,补不了的(UDP 连接跟踪、规则命中计数)激活 meow 时灰显标注「不支持」,不伪装支持。二进制分发:CI 构建时按钉在 package.json 顶层 `meowCoreVersion` 的版本号从 meow-rs 官方 release 下载 `x86_64-pc-windows-msvc` zip,解出 meow.exe 与 wintun.dll 作为第二 sidecar 打进 NSIS 安装包——安装包自包含可离线、bump 版本只改一处、构建可重现;二进制不进 git。服务模式与 sidecar 模式用同一套 `-d`/`-f` 参数托管 meow.exe,与 mihomo 无差别;内核重启本就由 fork 侧进程管理完成,不依赖 meow 缺失的 `/restart`。刻意不接入 meow 原生扩展(内置 web 面板、订阅管理、组 CRUD、metrics):verge 是唯一的面板与订阅事实源,双头管理只会制造分叉。

## Considered Options

- 推动上游接入或等上游回心转意:#7571 已关闭且无讨论,交付时间不可控,fork 自用等不起,弃用。
- 从 mihomo 或 sing-box 裁剪出自建轻量内核:裁剪本身可行,但要长期背编译配置与上游 rebase 的维护成本;meow-rs 已把 mihomo 兼容性当产品目标并维护兼容性文档,复用成本远低于自裁,弃用。
- meow 挂在「实验性通道」而非与 mihomo 平级(比如塞进 Alpha 同级的第三个隐藏选项):会把切换流程与能力差异做成两套逻辑;平级项 + 内核能力模型用一条路径覆盖全部内核,弃用。
- 接入 meow 原生面板 / 订阅管理 / metrics:verge 已有全套 UI 与订阅管线,再接一份等于双头管理订阅与分组,状态漂移无从对账,弃用。
- meow 二进制随 git 仓库分发(提交进 assets):仓库体积随每周发版膨胀,git 历史不可回收;钉版 + CI 下载同样保证可重现,弃用。
