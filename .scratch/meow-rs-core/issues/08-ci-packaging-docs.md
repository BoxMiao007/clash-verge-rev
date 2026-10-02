# 08: CI 打包接线 + 文档收尾

**What to build:** Windows 构建管线接入 meow 二进制:测试包工作流(win-x64-test-build)与正式版工作流(fork-release)构建时按仓库里钉死的版本号从 meow-rs 官方 release 下载 `x86_64-pc-windows-msvc` zip,解出 meow.exe + wintun.dll 作为第二 sidecar 打进安装包(二进制不进 git)。文档收尾:双内核架构与测速 IN-PORT 适配两篇 ADR,AGENTS.md 功能清单登记。本票交付后,其余工单的 Windows 实测才有安装包可用。

**Blocked by:** 02(sidecar 命名、版本钉位、下载形态在 02 定型)

**Status:** done

- [ ] 测试包工作流出包成功,安装包含 meow.exe 与 wintun.dll,离线 Windows 机器安装即用 —— **待推送验证**(工作流改动不 push 无法真跑;本地可验部分见 Comments,均通过)
- [ ] fork-release 工作出包成功(同上) —— **待推送验证**
- [x] meow 版本号钉在仓库单一位置,bump 只改一处,构建可重现 —— package.json 顶层 `meowCoreVersion: v0.21.2`,`scripts/prebuild.mjs` 读该值拼 release URL,两个工作流与 CI 无第二处版本号
- [x] ADR:双内核架构、测速 IN-PORT 适配 —— `docs/adr/0003-meow-rs-peer-second-core.md`、`docs/adr/0004-speedtest-inport-adaptation-for-meow.md`(01 已定案不回退,无需回退决策)
- [x] AGENTS.md 功能清单登记 meow-rs 第二内核 —— 已登记,引入提交留待合并 dev 时补
- [x] 验证门通过:`pnpm typecheck && pnpm test`(60/60)与 `src-tauri` 下 `cargo check` 全绿

## Comments

**Windows 侧声明核对(02 遗留面):** 对照后确认 tauri.conf 无缺——base `tauri.conf.json` 的 `externalBin` 已含 `sidecar/verge-meow`,`tauri.windows.conf.json` 未覆写该键(Windows 继承),`bundle.resources: ["resources"]` 整目录随包。真正的缺口有两处,已最小补齐:

1. `scripts/prebuild.mjs` `resolveMeowSidecar`:Windows 目标下从 meow zip 追加提取 `wintun.dll` 到 `src-tauri/resources/`(meow 按自身所在目录搜索该 DLL,必须与 verge-meow.exe 同目录);跳过/清理逻辑与 sidecar 同步,Linux tar.gz 无此文件不提取。
2. `src-tauri/packages/windows/installer.nsi`:资源只能落 `$INSTDIR\resources\`,安装段把 `resources\wintun.dll` Rename 到安装根与 verge-meow.exe 同目录(先 Delete 旧文件防升级期目标已存在),卸载段补对应 `Delete`。

**工作流本身无功能性改动:** `pnpm run prebuild x86_64-pc-windows-msvc` 一步在 02 已带上 verge-meow 任务,本票补上 wintun 提取后即覆盖全部所需;两工作流仅把步骤名改为 `Download sidecars (mihomo, meow)` 并注明版本来源,便于 CI 日志排查。

**本地实证(不依赖 push):**

- 以 `x86_64-pc-windows-msvc` 为目标干跑 prebuild:从钉版 URL 下载 `meow-v0.21.2-x86_64-pc-windows-msvc.zip`,产出 `sidecar/verge-meow-x86_64-pc-windows-msvc.exe`(14220288 字节)与 `resources/wintun.dll`(427552 字节),与官方 zip 内容清单逐字节一致;按 host(Linux)目标干跑确认不提取 wintun、Linux 路径不受影响。
- actionlint 1.7.7 对两个工作流 0 告警;三个 tauri conf JSON 校验通过(cargo check 过程中 tauri-build 对 base 配置另做真解析)。
- 服务模式的 meow staging(installer.nsi `--install-core`、prebuild core_hashes 哈希)不属本票,随工单 04 落地;届期服务目录的 meow 副本也需 wintun.dll 同目录。

**遗留:** wintun.dll 的 `LICENSE.wintun.txt` 未随包分发,合规如需可后续补一行提取。
