# Agent 开发约定(本 fork)

本文件面向在本仓库工作的 AI agent,由 fork 维护者维护,与上游无关。
上游同步合并时,本文件若与上游版本冲突,一律保留本文件版本,不要恢复上游内容。

## 分支模型

- `dev` 是集成分支:只接受两类提交——合并上游 `upstream/dev`、合并功能分支。全局文档改动(如本文件)可直接提交在 `dev`。
- 新功能一律在 `feat/<功能名>` 短命分支上开发,完成后合回 `dev`。
- 推送目标始终是 `origin`(自己的 fork),绝不向 `upstream` 推送。

## 上游同步

- 节奏:每 1~2 周或上游发版后同步一次,不长期积压。
- **合并前:**
  - 工作区必须干净;有未提交改动先提交或 stash,并向用户说明放到了哪里。
  - 显式 `git fetch upstream`,不用 `git pull`。
  - 先总结 `upstream/dev` 新提交,并对照下方「功能清单」标注可能与本 fork 功能重叠的文件。
  - 若发现上游历史被重写(上次合并点不再是 `upstream/dev` 的祖先),停下向用户确认,不强行合并。
- **合并中:**
  - 冲突逐个解决:先弄清两边意图再融合;本 fork 已有功能不可被丢弃或绕过;禁止为省事整体取单边。
  - 无法确定取舍时停下向用户确认,不猜。
  - 本文件(`AGENTS.md`)冲突一律保留本 fork 版本。
  - 合并搞砸可 `git merge --abort` 完整回到合并前状态,已提交的内容不受影响。
- **合并后:**
  - 依赖文件有变更(package.json / pnpm-lock.yaml / Cargo.toml / Cargo.lock)先 `pnpm install` 再验证。
  - 验证门:`pnpm typecheck && pnpm test`,以及 `src-tauri` 目录下 `cargo check`。全部通过才算完成。
  - `pnpm build` 是完整打包(Rust 全量编译,慢),只在需要产出安装包时执行,不作为每次同步的验证步骤。
  - 验证门通过后做功能巡检:用 `git diff <上一个 sync 标签>..upstream/dev --name-only` 列出本次同步引入的上游改动,对照「功能清单」的关键文件,输出受影响功能报告——波及哪些功能、判断依据、建议用户在应用里实际验证的点;没有历史标签时以本次合并的 merge-base 为基准。
  - 巡检完成后打标签 `sync/<YYYY-MM-DD>`,与代码一起推送到 `origin`(只能推 `origin`)。

## 测试包构建(Windows x64)

给维护者出人工验收用的 Windows x64 安装包时,用 dev 上的 `win-x64-test-build.yml` 工作流,不在本地打包。

- 被测分支需自带:工作流文件(GitHub 要求 dispatch 的 ref 上存在同名工作流)+ 测试变体配置(Cargo.toml 默认 features 加 `verge-dev`、tauri.conf 用 `.dev` identifier 且 `bundle.targets` 限 `nsis`),参考 `ci/win-test-build` 分支。
- 触发:`gh workflow run win-x64-test-build.yml --repo BoxMiao007/clash-verge-rev --ref <被测分支>`;`gh` 不带 `--repo` 会打到上游。
- 产物在 Actions artifact,实际路径为仓库根 `target/release/bundle/nsis/*.exe`(workspace 布局,不在 src-tauri/target)。
- 构建命令只有 `pnpm build`:tauri CLI 参数经 `pnpm --` 转发会误递给 cargo;verge-dev 与 nsis 走分支配置,不走 CLI 参数。
- 测试包为 `.dev` 身份,数据目录/单实例/系统服务均与官方安装版隔离,可并存;首次启动需重新导入订阅。
- CI 签名密钥在 fork secrets(`TAURI_PRIVATE_KEY`/`TAURI_KEY_PASSWORD`),一次性密钥,仅为满足 `createUpdaterArtifacts` 的构建签名要求,与官方更新链无关;丢失则 `pnpm tauri signer generate` 重新生成后 `gh secret set`。

## 正式版发布(Windows x64)

fork 正式版用 dev 上的 `fork-release.yml` 工作流发布:官方 identity、仅 Windows x64,tag 推送 `v*-fork.*` 自动触发。

- tag 命名 `v<上游基线>-fork.<序号>`(如 `v2.5.7-fork.1`),在 dev 上打并推送;CI 内把应用版本盖成 build metadata 形式的 `2.5.7+fork.1`(`release-version.mjs` 只接受这种 fork 后缀),不污染 dev 版本号。
- 上游同步合入新基线后即可出包:首个 tag 用 `v<新基线>-fork.1`,同一基线的后续重发递增序号(fork.2、fork.3…);是否随同步出包由维护者决定,不是同步的固定步骤。
- 工作流文件必须存在于被发布 tag 的提交里:先合入 dev 再打 tag。
- 重发同一 tag:`gh workflow run fork-release.yml --repo BoxMiao007/clash-verge-rev -f tag=v2.5.7-fork.1`。
- 与官方版数据兼容(同 identity,覆盖安装不迁移数据);应用内更新已禁用(ADR-0002),升级 = 下载新安装包覆盖安装,「检查更新」按钮报错属预期。

## 功能清单

本 fork 相对上游的自有功能登记处:新功能合入 `dev` 时必须登记一行,功能下线时移除。agent 会话以此了解本 fork 有哪些功能;设计背景见 `.scratch/<feature>/` 的 spec,代码演变见 git 历史(`git log upstream/dev..dev --no-merges`)。

| 功能 | 一句话说明 | 关键文件/入口 | 引入提交 |
| ---- | ---------- | -------------- | -------- |
| 节点下载测速 | 整组/单项限时下载测速,GLOBAL 专用通道不干扰当前选择,支持按速度排序与自定义 URL/时长/流量上限 | `src/services/speed.ts`、`src-tauri/src/feat/speedtest.rs`、代理页(按钮与排序)、设置页速度测试卡片(`src/components/setting/setting-verge-speedtest.tsx`) | `037da98a` |

## 降低合并冲突的约定

- 能通过新增文件接入的功能,不修改上游既有文件。
- 提交信息写清改动意图(做什么、为什么),便于合并冲突时判断取舍。
- 本 fork 内提交信息用简体中文;如向上游提交 PR,遵循上游仓库的契约(英文提交、Conventional Commits 等)。

## Agent skills

### Issue tracker

Issues 以本地 markdown 文件形式存放在 `.scratch/<feature>/`。见 `docs/agents/issue-tracker.md`。

### Triage labels

默认五角色词汇表(needs-triage / needs-info / ready-for-agent / ready-for-human / wontfix)。见 `docs/agents/triage-labels.md`。

### Domain docs

单上下文布局:根目录 `GLOSSARY.md` + `docs/adr/`,按需懒创建。见 `docs/agents/domain.md`。
