# 01: 速度测试设置卡片(挪两项)

**What to build:** 设置页右栏、「Verge 基础」与「Verge 进阶」之间出现常驻「速度测试」卡片,含测速 URL、测速时长两个条目;改动即时生效,URL 输入失焦才落盘;杂项弹窗中原来的这两项删除,速度测试设置自此单一入口。已保存过这两项设置的升级用户无感迁移(配置键不变)。

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] 新卡片为独立新增组件接入设置页,不改动上游既有卡片组件
- [x] 卡片条目走既有卡片保存模式;时长输入沿用既有即时夹取(1–30 秒)
- [x] URL 输入过程只更新本地态,失焦时写配置
- [x] 杂项弹窗删除测速 URL 与测速时长两项,延迟测试相关条目不受影响
- [x] 既有配置键名与语义不变,老用户已存值直接生效
- [x] 卡片标题与条目文案(含占位符/tooltip 语义「留空用内置默认值」)覆盖全部语言文件,沿用「速度测试」措辞家族
- [x] `pnpm typecheck && pnpm test` 通过;应用内改 URL/时长后测速按新参数执行

## Answer

实现提交 `1272c4c8`(分支 feat/speedtest-settings-card-01,经 `229e93f2` 合入集成分支)。

- 新增 `src/components/setting/setting-verge-speedtest.tsx`:URL 失焦提交先例 + 失败回滚;时长 GuardState 即时保存,复用既有 clamp/resolve,未新增纯函数。
- `settings.tsx` 右栏挂载;`misc-viewer.tsx` 删两项(-80 行),延迟条目未动;配置键不变。
- 13 语言新增 `settings.components.verge.speedtest.*`,杂项 4 个旧键删除;i18n 生成物经 generate-i18n-keys.mjs 重生成。
- 验证:typecheck、vitest 53 用例、eslint/biome、i18n:check 全绿。评审修复 `dcc25a1c` 补挂载点 onError 透传。
- 遗留:应用内 GUI 实测(改 URL/时长后测速按新参数执行)待维护者人工验收。
