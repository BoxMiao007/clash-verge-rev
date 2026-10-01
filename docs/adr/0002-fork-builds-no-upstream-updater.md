# 0002 - fork 构建不使用上游更新链

fork 的正式版与 autobuild 构建刻意使用官方 identity(与官方安装版共享数据目录,数据兼容是卖点),但 tauri.conf.json 里的更新端点与签名公钥原本指向上游官方仓库:fork 构建装上后点「检查更新」,会拉到上游更高版本的官方安装包直接覆盖安装,fork 自有功能被整体冲掉,且维护者很难第一时间意识到原因。决定:fork 构建移除 `plugins.updater` 配置并关闭 `createUpdaterArtifacts`——应用内更新彻底禁用(后台静默检查失败只落日志不弹窗,「检查更新」按钮报错属预期),升级一律通过发布新 tag 的安装包手动覆盖;fork 的签名密钥(TAURI_PRIVATE_KEY/TAURI_KEY_PASSWORD)只为满足测试包构建的签名要求,与任何更新链无关。若未来要恢复应用内更新,属明确的重建决策:重新生成密钥对、公钥与端点指向 fork 自己的 updater release、每版维护 latest.json,不是把配置回填。

## Considered Options

- 更新端点指向 fork 自己的 release 并维护 latest.json:保留应用内更新体验,但 fork 现有一次性密钥无法导出对应公钥,需整对重新生成,且每版发布多一步更新元数据维护,单人 fork 不划算,弃用。
- 保留上游端点不动:零改动,但等于给所有 fork 构建埋了「一次点击即被官方版覆盖」的雷,且与「fork 自有功能」的存在前提直接冲突,弃用。
- 仅在发布工作流里临时 patch 配置:dev 与 autobuild 构建仍带上游端点,雷只拆了一半,配置与仓库不一致也难排查,弃用。

## 修订(2026-10-01):移除整个 `plugins.updater` 会导致应用无法启动

v2.5.7-fork.1 把 `plugins.updater` 整块删除,结果正式包装上后应用秒退(无窗口、无输出、退出码 1),本地复现实证:`builder.build()` 阶段退出码 1 静默终止。原因:tauri 核心对缺失的插件配置键传 `Null`(tauri `plugin.rs` 的 `unwrap_or_default()`),适配器用 `serde_json::from_value` 转类型化 Config 并在失败时让整个应用构建失败;而 updater 的 `Config` 自定义反序列化要求 `pubkey` 必填——整块删除传 Null、甚至空对象 `{}` 都会炸。正确禁用姿势是保留配置但清空能力:`"updater": { "pubkey": "", "endpoints": [] }`,插件初始化通过,所有更新检查因无端点而优雅失败(静默检查只落日志,按钮报错)。

### Considered Options(补充)

- 空对象 `"updater": {}`:以为字段都有默认值,实际 `pubkey` 无 `#[serde(default)]`,反序列化同样失败,弃用。
- 用 cargo feature 在 fork 构建下不注册 updater 插件:需要改上游 `lib.rs` 的插件注册代码,扩大合并冲突面,收益只是省三行配置,弃用。
