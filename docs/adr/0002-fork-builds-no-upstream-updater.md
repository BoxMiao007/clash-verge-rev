# 0002 - fork 构建不使用上游更新链

fork 的正式版与 autobuild 构建刻意使用官方 identity(与官方安装版共享数据目录,数据兼容是卖点),但 tauri.conf.json 里的更新端点与签名公钥原本指向上游官方仓库:fork 构建装上后点「检查更新」,会拉到上游更高版本的官方安装包直接覆盖安装,fork 自有功能被整体冲掉,且维护者很难第一时间意识到原因。决定:fork 构建移除 `plugins.updater` 配置并关闭 `createUpdaterArtifacts`——应用内更新彻底禁用(后台静默检查失败只落日志不弹窗,「检查更新」按钮报错属预期),升级一律通过发布新 tag 的安装包手动覆盖;fork 的签名密钥(TAURI_PRIVATE_KEY/TAURI_KEY_PASSWORD)只为满足测试包构建的签名要求,与任何更新链无关。若未来要恢复应用内更新,属明确的重建决策:重新生成密钥对、公钥与端点指向 fork 自己的 updater release、每版维护 latest.json,不是把配置回填。

## Considered Options

- 更新端点指向 fork 自己的 release 并维护 latest.json:保留应用内更新体验,但 fork 现有一次性密钥无法导出对应公钥,需整对重新生成,且每版发布多一步更新元数据维护,单人 fork 不划算,弃用。
- 保留上游端点不动:零改动,但等于给所有 fork 构建埋了「一次点击即被官方版覆盖」的雷,且与「fork 自有功能」的存在前提直接冲突,弃用。
- 仅在发布工作流里临时 patch 配置:dev 与 autobuild 构建仍带上游端点,雷只拆了一半,配置与仓库不一致也难排查,弃用。
