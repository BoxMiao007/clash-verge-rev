# 03: 测速设置项(测速 URL 与测速时长)

**What to build:** 用户在设置弹窗中可修改测速 URL 与测速时长(秒),默认值 googlechrome.dmg 与 5 秒;仿照现有「默认测试链接 + 默认延迟超时」的成对模式存入 verge 配置(patch 注册、TS 类型同步),设置 UI 放在同一弹窗、紧邻延迟设置。SpeedManager 与后端命令改为读取设置值,保存后对下一次测速立即生效。非法输入(空 URL、超范围时长)有校验或默认值兜底。

**Blocked by:** 02(命令签名与 SpeedManager 已接收参数)

**Status:** ready-for-agent

- [ ] 设置弹窗新增两个字段,保存后下一次测速即按新值执行
- [ ] 未配置时使用默认值;非法输入不产生崩溃或无效请求
- [ ] 配置结构、patch 注册、TS 类型三处一致
- [ ] zh/en 文案就位
- [ ] `pnpm typecheck && pnpm test` 与 `cargo check` 通过
