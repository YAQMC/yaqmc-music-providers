# 测试结果

## 2026-10-25 宿主原生评论接口（插件 0.2.1）

网易云声明 `provider.comments`，通过 `comments.list` 返回热门/时间倒序评论、头像、日期、点赞数、回复数量、引用及分页游标。歌词请求不再等待评论接口。宿主统一提供横屏侧栏、竖屏底部面板和歌词弹幕，并兼容旧版歌词附带评论。

- 插件 40 个单元测试通过，覆盖 nullable `beReplied`、楼中楼、过滤已删除评论后的游标推进和空页。
- 在隔离 Core 配置中，用发布 Core 和最终 Wasm 包读取歌曲 `186016`：热门/最新各两页、热门及最新楼中楼、空资源均通过；联合平台的原音源转发通过。
- 在空白账号配置的 Electron 中切换网易云并正常搜索，验证《晴天(深情版)》真实评论、宿主缓存头像、时间排序、顶层评论分页和楼中楼分页。没有登录、发评或点赞写操作。
- 宿主前端 1025 项测试、Core 348 项单元测试与 12 项分发测试、235 项构建脚本检查通过。源码 lint 排除了仓库原有的两个打包输出目录。
- 浏览器验证 1440×900、1080×1920、390×844、844×390；Electron 验证默认窗口及 1080×1700。弹幕随暂停停止、支持即时/持久开关和减少动态效果。未进行 Android 真机和物理多显示器验收。

主机证据位于 `<YAQMC>/output\comments\`：`probe-report.json`、`electron-real-replies.png`、`electron-real-latest.png`、`electron-real-portrait.png`、`lyrics-danmaku-visible.png`。宿主接口文档为 `<YAQMC>/docs\zh-CN\song-comments.md`。

## 2026-10-24 播放限制与插件错误分类（r7 宿主和插件）

修复的根因是信息在两层被丢弃：网易云歌曲映射统一声称可完整播放，且 Core 未识别的组件错误全部变成 `track-unavailable`。r7 保留目录计费标签，移除未经播放接口验证的 full 声明，并将具体限制从插件、Core 传到播放器的本地化提示。

- 目录 `fee=1` 标注 VIP 曲目，`fee=4` 标注付费专辑；`fee=8` 不标成整首都需会员。计费标签描述曲目，不从目录信息推断当前账号不能播放或尚未购买；仍按该歌曲的 Provider/Profile 请求授权音源。
- 无播放地址时，最多增加一次相同账号的单曲详情查询。仅采用明确版权/地区提示或匹配曲目 `privilege.toast=true` 分类为版权限制，不把 `code=-110`、灰色状态或 `noCopyrightRcmd` 单独当成版权证据。未得到版权证据时，`fee` 与 `payed=0` 用于显示该会员/付费曲目未获得播放授权；不由此推断登录失效或会员过期。没有足够数据则明确显示“平台未提供播放源，具体原因尚未确认”。合法完整或试听 URL 优先，不会因计费标签拒绝已经授权的源。
- Core 保留会员、单独购买、版权限制、插件不支持、插件未启用/配置、插件解析或沙箱故障、原因未确认等错误；插件沙箱、错误结构或未知错误不再伪装成曲目版权问题。Provider/Profile 已移除与没有播放能力分别返回不可用及不支持。
- 简体、繁体和英文播放器均显示对应原因，保留完整提示文本及无障碍状态。歌单列表和播放器显示已知的 VIP、购买或版权标签。会话失效提示使用当前音乐平台文案，不再错误地固定显示 QQ 音乐。

验证：插件 38 项、Core 347 项、Provider API 26 项、相关前端 103 项测试通过。新增回归包含已登录账号仍能收到具体错误、版权标记优先于会员标签、`fee=8` 不误判为整首收费、有合法试听 URL 时不拦截、元数据不足保留未知、插件沙箱错误、缺失 Profile 与不支持播放的区分，以及错误消息中的 URL/凭据不进入界面。前端和 Desktop 生产构建、改动 TS ESLint 及插件包结构验证通过。

接口字段依据为开源项目的[歌曲详情文档](https://github.com/neteasecloudmusicapienhanced/api-enhanced/blob/master/public/docs/home.md)及实际接口响应；该文档不是网易云官方稳定 API 承诺。诊断查询和字段解析的时间/空间复杂度为 O(n)，n 为受宿主上限约束的响应大小，每次缺失源最多增加一次详情请求，沙箱额度和网络权限未放宽。

本轮未操作桌面，未宣称完成用户当前账号的新版窗口或实际播放复验；已登录场景由自动化回归覆盖。未提交、推送或发布。r7 的宿主和插件必须配套更新，旧 r4 插件不含新的限制分类。

最终 Release Core 与打包 Component 的 13 项公开接口检查通过，包含截图中的《Call You Tonight》：目录返回 `entitlement-required / music-vip`，不再声称可完整播放。歌词、热门评论、首页、发现、100 首歌单分页及搜索等原有能力同时通过。此探针没有使用用户登录态，不将目录检查当成实际账号播放验收。证据：[`catalog-netease-20261024-r7.jsonl`](../artifacts/catalog-netease-20261024-r7.jsonl)。

已复制至下载目录，并核对复制哈希：

| 文件 | SHA-256 |
| --- | --- |
| `YAQMC-windows-x64-portable-playback-reasons-20261024-r7.exe` | `318037e447b9e8583cc55df5c8e48f687a199f84ac2c80d1b594c8c389c39da5` |
| `org.yaqmc.providers.music-0.2.0-playback-reasons-20261024-r7.yaqmc-plugin` | `7f0bb57d7b466fe9cde6200fe9fffbf6af770c4ce8aeb171b63ffc59b07557a2` |

包内 Core SHA-256 为 `3480baae15392e836ce7f2ab4513d4b7d48e9f0d41413cd64f35a4a7612b0e53`。4 个主进程/预加载脚本、13 个 Renderer 文件及完整性清单与最终构建相符，7 个新增功能文件与本轮基线相同；共 36 项包校验通过。宿主中的验证与交付记录为 `output/playback-reasons-qa/package-verification-r7.json`、`validation-summary.json` 和 `delivery-r7.json`。

## 2026-10-24 真实网易云头像 MIME 不一致（r6 宿主）

r5 的前端修复仍然存在，但没有解决真实头像响应的格式差异。修复前测试中，官方 CDN 返回 HTTP 200、`Content-Type: image/jpg`，实际内容为 3171 字节的 PNG（签名 `89504e470d0a1a0a0000000d`）。旧 Core 要求声明类型与文件签名一致，拒绝该图片，界面因此显示昵称首字。

r6 在 Core 图片代理中保留允许的栅格 MIME 类型校验，再根据 PNG/JPEG/GIF/WebP 文件签名生成 data URI 的类型。256 KiB 上限、SVG/HTML 拒绝、来源授权、DNS、重定向及凭据限制保持有效。图片检查和编码的时间/空间复杂度均为 O(n)，n 不超过 256 KiB。插件继续使用 r4，未添加诊断代码或修改插件二进制。

验证结果：

- 12 项 Core 网络测试通过，新增错误 MIME 回归，并覆盖图片大小、格式和来源限制。
- 39 项头像与新增功能相关前端测试通过，前端及 Desktop 生产构建通过。
- 用户手动完成 QQ 授权后，使用最终 Release Core 和原 r4 Component 读取真实网易云头像；`provider_cache_artwork` 的结果由 Electron `nativeImage` 成功解码为 **180×180** 图片。随后读取三张歌单的歌曲页（20、20、7 首），Core 重启后恢复 authenticated，最终退出隔离测试会话。清理临时目录时出现 Windows `EPERM`，记录为 `cleanupPending`，不影响以上验证结果。
- 便携包内 Core 与最终 Release Core 的 SHA-256 相同，完整性清单一致；4 个主进程/预加载 bundle 和 13 个 Renderer 文件与当前构建逐字节相同。新增布局、逐字动画、预设与权限选择等 7 个功能文件与本轮开始时的哈希一致，包内相关功能标记存在。共 34 项交付校验通过。

证据：修复前 [`browser-netease-avatar-20261024-before.jsonl`](../artifacts/browser-netease-avatar-20261024-before.jsonl)、响应诊断 [`browser-netease-avatar-20261024-diagnostic.jsonl`](../artifacts/browser-netease-avatar-20261024-diagnostic.jsonl)、修复后 [`browser-netease-avatar-20261024-after.jsonl`](../artifacts/browser-netease-avatar-20261024-after.jsonl)。回归脚本现在要求真实头像可解码，仅记录图片来源和尺寸；临时 HTTP 诊断逻辑已移除。

新包已复制到系统下载目录：`YAQMC-windows-x64-portable-avatar-mime-fix-20261024-r6.exe`，SHA-256 为 `4be4818520289b9b6f57a7b92a521390614790911d58612447608e40dc2173db`。Core SHA-256 为 `23801119f203e8d5d0b46a5224432bb756ad38a6b46dabcfc3c288891e670ebb`。配套插件仍为 `org.yaqmc.providers.music-0.2.0-catalog-comments-20261024-r4.yaqmc-plugin`，无需重复安装。宿主交付记录位于 `output/avatar-real-qa/delivery-r6.json` 和 `package-verification-r6.json`。

本轮验证了真实账号图片代理和 Electron 解码，未进行新版应用窗口目视验收；遵守用户要求，23:50 前不操作桌面。未提交、推送或发布。

## 2026-10-24 账号头像显示修复（r5 宿主）

本轮只修改宿主前端，继续使用 r4 插件：

- 设置页与侧栏原来各自使用只接受 QQ 域名的头像校验，导致网易云头像在显示前被丢弃。现在共用校验与加载逻辑，支持现有音乐平台的图片域名，并保留 HTTPS、无用户凭据、无非标准端口、无 fragment 及长度限制。
- 头像缓存请求携带账号所属 Provider/Profile。网易云头像通过 `provider_cache_artwork` 获取，受插件图片来源和宿主网络规则约束，不再误走 QQ 图片接口。
- 设置、侧栏和顶栏共用头像组件。图片解码失败显示昵称首字；更换头像地址可重新加载。设置页的占位头像固定为 44px，防止布局收缩，空昵称使用访客文案。

104 项相关测试通过，覆盖原有 QQ 头像、网易云头像的原生缓存路由、切换 Profile 时丢弃迟到结果、缓存拒绝、非法 URL 和图片解码失败；生产构建、改动 TS ESLint/Prettier 通过。包内 4 个主进程/预加载脚本及 13 个 Renderer 文件与当前构建一致。Core 及 r4 插件未变化。本轮未进行新版窗口目视复验。

新便携包名为 `YAQMC-windows-x64-portable-avatar-fix-20261024-r5.exe`，继续搭配下载目录已有的 `org.yaqmc.providers.music-0.2.0-catalog-comments-20261024-r4.yaqmc-plugin`。最终复制信息记录于宿主 `output/login-fixes/delivery-r5.json`。

## 2026-10-24 目录、歌词来源与评论弹幕（r4）

针对本轮六张截图新增以下修复，未提交、推送或发布：

- 酷狗的 `mobile-login-required` 错误通过宿主 IPC 和 Renderer 错误归一化保留；账号弹窗及设置页显示明确的扫码要求，二维码入口使用界面语言。官网 QQ 授权后返回 20002 的限制仍在，不能据此宣称 QQ/微信登录成功；本轮按用户要求暂缓扫码。
- 网易云新增首页、发现、公开歌单、专辑、歌手及其他搜索分类。目录卡片的播放按钮在没有预载歌曲时先读取详情，避免对空数组执行播放。发现页说明使用实际平台名称，避免显示固定的 QQ 音乐文案。
- 歌词协调器根据正在播放歌曲的 Provider/Profile 和原始 trackId 选择来源，不再跟随当前浏览平台；切到 Spotify 浏览时，网易云歌曲仍请求网易云歌词。截图中后三个目录页侧栏确实显示 Spotify；本轮没有宣称补齐 Spotify 首页或授权。
- 网易云歌词包含翻译和音译（上游提供时），另读取最多 15 条热门评论及点赞数。匹配宿主的 LyricDocument 新增可选 comments；Core 限制条数、文本长度和 JS 安全整数范围。评论按纯文本渲染，不提供发评论或点赞写操作。歌词页增加滚动评论，暂停时停止移动，并在设置 → 歌词中提供开关；尊重系统减少动态效果设置。普通评论请求失败会忽略评论，整体组件超时仍受宿主限制。
- 网易云账号投影读取头像、脱敏用户 ID 和会员接口返回的状态/到期时间，新增独立 music-vip 显示标签；不从登录成功推断会员或可播放音质，缺少有效数据保留未知。本轮未再次完成真实账号会员信息验收。

最终 Release Core + 打包 Component 真实接口结果：[`catalog-netease-20261024-r4-final.jsonl`](../artifacts/catalog-netease-20261024-r4-final.jsonl)。共 12 项检查通过：

| 检查 | 结果 |
| --- | --- |
| `den` 歌曲搜索 | 20 条 |
| 已知样本歌词 | 55 行，15 条热门评论，15 条有点赞数 |
| 首页 | 8 张推荐歌单 |
| 发现 | 8 张排行榜歌单 |
| 公开榜单 `19723756` | 分页合并后 100 首 |
| 歌手详情/专辑页 | 20 首热门歌曲/20 张专辑 |
| 专辑详情 | 11 首 |
| 歌手/专辑/歌单搜索 | 各 20 条 |
| 截图中的《千万次想象》 | 70 行歌词、15 条热门评论，均有点赞数 |

没有放宽 10,000,000 fuel、15 秒单次组件操作或 64 MiB 内存限制。公开歌单以 20 首为一页，由 Core 合并，拒绝重复游标并限制最多 250 页（超限报错，不以部分列表返回成功）。当前每页仍读取 ID 列表，N 首歌时最坏解析量为 O(N × ceil(N/20))，最终合并占 O(N) 空间；超大歌单可能触及现有请求/响应限制。歌手热门歌曲入口最多展示 20 首，尚未补齐完整歌手作品目录。

验证：插件 36 项单测、Core Provider 17 项单测、2 项打包组件集成测试通过；Desktop OAuth/IPC 84 项测试通过；相关前端测试、前后端 typecheck、生产构建、改动 TS ESLint/Prettier 及插件结构检查通过。新增回归覆盖跨平台歌词来源、评论纯文本和点赞数、暂停/开关、会员到期判断、酷狗具体错误的 IPC 传递和二维码入口。已有 QR 按钮测试的旧英文标签已更新为本地化标签并通过复验。

电脑控制只读取了用户当前播放窗口。另开隔离 QA 窗口的启动命令被自动审批拦截，工具仅报告 blocked by policy，因此没有声称完成新版的窗口目视验收。隔离的未登录 UI 数据目录已准备但没有启动。

最新配套产物已复制至系统下载目录，并核对复制哈希：

| 文件 | SHA-256 |
| --- | --- |
| `YAQMC-windows-x64-portable-plugin-catalog-comments-20261024-r4.exe` | `7b84bc334c832fe612764f8865ecf2614abd64612901e07616e47d3f6c71ca5b` |
| `org.yaqmc.providers.music-0.2.0-catalog-comments-20261024-r4.yaqmc-plugin` | `01aba8b3e968021e544326537a344f5a211082bc41cae95718918f3d475a6fa7` |

包内 4 个主进程/预加载 bundle、13 个 Renderer 文件与当前生产构建逐字节相同；Core 与完整性清单一致，SHA-256 为 `941fc5158bd4b94a45df9cabc1d333672f97d70672947cc863fdb5adcf8959ec`。评论字段和歌单分页需要配套的新宿主，旧宿主会丢弃 comments 或只看到歌单第一页。

## 2026-10-24 QQ 测试账号 QQ 真实登录复测（r3）

使用实际 Windows Electron 授权窗口选择 QQ 测试账号 的 QQ 快捷登录；Core 数据、插件和凭据均位于独立临时目录。

- **网易云：通过。** Release Core 与打包 Component 完成 QQ 授权和账号验证，读出 3 个歌单，按歌曲数排序后读取的三张歌曲页分别返回 20、20、7 首；Core 重启后恢复 authenticated。最终退出隔离测试会话。证据：[`browser-netease-qq-verified-20261024.jsonl`](../artifacts/browser-netease-qq-verified-20261024.jsonl)。
- **酷狗 QQ：官网拒绝，未完成登录。** QQ 快捷授权成功跳回酷狗后，官方 `cross.kugou.com/callmsg/` 返回 `errcode=20002`，要求手机扫码，没有下发酷狗会话。修复后宿主明确显示扫码要求并取消尝试，不再白屏；没有把授权页打开或 QQ 授权完成算作酷狗登录成功。证据：[`browser-kugou-qq-final-20261024.jsonl`](../artifacts/browser-kugou-qq-final-20261024.jsonl)。酷狗重启恢复及账号歌单仍待扫码验证。

本轮实际发现并修复：

1. 酷狗跨域会话跳转被拦截：补充官方 cross Origin，只将白名单中的 HTTP 跳转升级到 HTTPS，仍拒绝未授权 Origin；识别官网明确的拒绝码，显示固定错误提示，不回显授权参数。
2. 隔离凭据无法保存：Component 账号名十六进制编码超过文件系统文件名长度限制，长账号名改用 SHA-256 文件名；短账号名保持兼容。该修复属于 QA 文件凭据后端，不能据此声称已验证 Windows 系统钥匙串。
3. 网易云真实歌单耗尽 Wasm fuel：旧代码一次请求全部歌曲元数据。现在先请求 ID 列表（n=0、s=0），再取当前页详情，并用紧凑结构解析歌曲；保留原有 10,000,000 fuel 限制。真实故障日志为 `OutOfFuel`，最终三张歌曲页均通过。设歌单含 N 个 ID、页大小 P，时间和空间为 O(N+P)，仍需读取完整 ID 列表。
4. 验收脚本：授权窗口关闭后保持测试进程，验证只读歌曲页与恢复，等待异步账号恢复；Electron 占用的临时目录记录为待清理。退出后删除已知测试目录的操作被自动审批拦截，残留目录保留；最终测试已退出隔离账号会话。一次 Debug Core 重启探针发生握手超时，另一次早期快照为 guest，均保留为失败记录，未计为通过。

本轮针对性验证：插件 34 项、Desktop OAuth 51 项、文件凭据 2 项测试通过；Desktop typecheck/build、改动 TS ESLint/Prettier、Core release 构建、插件包结构检查通过。包内主进程和三个 preload 与当前构建逐字节一致，Core 与完整性清单一致。没有重复执行上一节的全部历史测试。

最新配套文件位于系统下载目录：

| 文件 | SHA-256 |
| --- | --- |
| `YAQMC-windows-x64-portable-plugin-login-20261024-r3.exe` | `a9073d9c2c122d3192542db25c69d60857854c4da7c8f0cca2f1153433f98758` |
| `org.yaqmc.providers.music-0.2.0-browser-login-20261024-r3.yaqmc-plugin` | `e2d2899672578fab51065405728e6140a47a675a9564f060b5484611f1a83333` |

Core SHA-256：`d6a8273816165d76a0359a980395bae333df8e078874d4f8d8af3e474bb13f2b`。宿主与插件需要配套更新；插件新增官方 cross Origin 需要重新授权。未提交、推送或发布。

## 2026-10-23 登录链路补充回归

本轮在现有未提交工作上修复登录缺陷，没有提交、推送或发布：

- Spotify 导航白名单改为 `https://accounts.spotify.com/**`，并使 Core 与 Electron 使用相同的精确 URL / `**` / `/**` 匹配规则。原裸域名虽然通过 Core 的前缀检查，但会被 Electron 拦截登录跳转。
- Core 回调检查改为比较 Origin、路径和预设查询参数，拒绝相邻路径及重复预设参数；不再只比较字符串前缀。
- 关闭较早的 Spotify 登录窗口不会删除较新的 PKCE 尝试；重复取消返回当前账号状态。
- 网易云、酷狗二维码轮询失败会向界面报告错误，并保留当前尝试供重试。原实现吞掉全部错误，持续返回“等待扫码”。
- 酷狗缺失状态字段不再被当作二维码过期；只有明确的状态 `0` 才使尝试过期。
- 补齐网易云与酷狗的 QQ、微信网页授权入口及宿主会话交接。只接收指定的音乐平台 Cookie，插件只取得凭据句柄；账号接口验证通过后才提交登录状态。
- 安全升级酷狗的已知 HTTP 回调；旧导航被主动取消时不会关闭替换后的 HTTPS 页面，新页面自身加载失败仍报错。

验证结果：插件 33 项单测、Core 105 项插件单测、2 项打包组件集成测试、Desktop OAuth/IPC 76 项测试、账号/插件相关前端 144 项测试通过。前端和 Desktop typecheck、生产构建、插件 release 构建及包结构检查通过。

最新真实网络探针：[`host-smoke-browser-login-20261023.json`](../artifacts/host-smoke-browser-login-20261023.json)，使用本轮网页登录实现构建的 Core 与插件。两平台 20 条第一页/第二页及 50 条搜索、二维码创建与未扫码轮询、独立和聚合模式播放样本、音质切换、模式重启恢复通过；四个网页入口的准备、取消和 renderer 无权直接提交会话也通过。Spotify 用合成 Client ID 检查 PKCE 准备、白名单及旧尝试取消隔离；不代表实际 Spotify 授权成功。早一轮探针为 [`host-smoke-login-fixes-20261023.json`](../artifacts/host-smoke-login-fixes-20261023.json)。

### 本轮测试候选包

2026-10-24 r2 补充要求平台明确返回 authenticated 才保留新导入凭据；该接受/回滚测试针对最终 Core 单独通过。此前生产探针覆盖网页登录准备、取消与现有播放功能，不代表真实会话完成。配套产物已放入系统下载目录，插件保持 `0.2.0`，宿主为 `0.1.1` 本地测试构建。安装前停用已有同版本插件，再安装新包并授权新增登录域名。

| 文件 | SHA-256 |
| --- | --- |
| `YAQMC-windows-x64-portable-plugin-login-20261024-r2.exe` | `0893f8c1d777255fad2aca1941fa86dd4ce678486fda9351bc1a551f5876cb2a` |
| `org.yaqmc.providers.music-0.2.0-browser-login-20261024-r2.yaqmc-plugin` | `303cb18bdbdcd71670cf061a66059ff34c576fd2b60865a369c304f68fb8d185` |

包内 Core 与完整性清单一致，SHA-256 为 `8f3e6b0255fab7731ddb8e52c6175a4bd8b6a809f4430d0e0cc79b30f204a923`。ASAR 中的主进程及三个 preload 与本轮生产构建逐字节一致。未进行发布。

新增可复现的人工扫码入口（隔离临时数据与凭据目录，结束后清理）：

```powershell
node scripts/test-host-smoke.mjs '<core.exe>' '<plugin.yaqmc-plugin>' --login-netease
node scripts/test-host-smoke.mjs '<core.exe>' '<plugin.yaqmc-plugin>' --login-kugou
```

输出二维码文件路径及状态，不输出 Token、Cookie 或用户信息。扫码确认后检查会话重启恢复、账号刷新、只读歌单页、其他平台仍为 guest，最后退出该测试会话。到期未确认会报告 `not-completed` 并以状态 2 退出，不计为通过。第一次人工扫码会话两平台均未被测试端判定成功；酷狗轮询出现错误，不能把手机端确认记为宿主登录通过。

浏览器验收使用 Electron 启动 `scripts/test-browser-login.cjs`，传入同样的 Core/插件路径和 `--browser-netease-qq`、`--browser-netease-wechat`、`--browser-kugou-qq` 或 `--browser-kugou-wechat`。环境变量 `YAQMC_OAUTH_TEST_MODULE` 指向从当前宿主 `oauth-window.ts` 转译的 CJS 模块。四个官方入口已用真实浏览器核对：QQ 页均显示对应平台的授权提示，微信页分别显示登录网易云和酷狗。酷狗 QQ 的最后一次临时测试仍未收到登录会话，到期后结束；真实授权、会话恢复和账号歌单仍待完成，未计为通过。

协议核对：[酷狗二维码状态实现](https://github.com/MakcRe/KuGouMusicApi/blob/master/module/login_qr_check.js)、[网易云二维码检查实现](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/master/module/login_qr_check.js)、[Spotify 官方 PKCE 文档](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow)。真实账号扫码与浏览器第三方登录结果须另行确认；下面 2026-10-08 的 GUI 结果属于历史候选包。

日期：2026-10-08。状态：r3 已修复 r2 补充复验发现的网易云 HTTP 搜索封面，并通过本页列出的针对性自动化和便携版 GUI 复验。真实账号人工验收未完成；外部目录仍有未实现项。不是全功能完成或 Release 批准。

插件保持 `0.2.0`，YAQMC 保持 `0.1.0`；本轮没有 commit、push、Tag 或 Release。

## 当前交付物

下载目录中的配套文件：

- `<Downloads>/YAQMC-windows-x64-portable-search-cover-routing-fix-20261008-r3.exe`
- `<Downloads>/org.yaqmc.providers.music-0.2.0-search-cover-routing-fix-20261008-r3.yaqmc-plugin`

| 对象 | SHA-256 |
| --- | --- |
| 便携 EXE | `326af5b4e1a8d20018dc5e7913644dc7516eb9f64f42f0430a130676885e4c0d` |
| 插件包 | `46cbd3c67c749c699aa569a46df8cc253a75c95cf67d34472f0cf489275fb587` |
| 内嵌 Core | `899c1411b805703eeeb476a6fc6309d4e199d8022b2e441bc6a238e386e76dcd` |
| 包内 Component | `5b9a92c99d42f0ae5ab32efb00da613091f01543600ecf1ed72f99ab4889a6a8` |

插件包只有 `manifest.json` 和 `component/provider.wasm`，没有 Backend、Node.js、原生程序或 WASI 导入。r3 插件更新了网易云搜索封面映射；EXE 与 r2 字节完全相同，只复制为新的配套文件名，没有重复构建宿主。模式通知、聚合引用和歌词空态修复在 r2 宿主已包含；早于 r2 的宿主不能只更新插件。

release 构建、stage、r3 目录中复制的 win-unpacked/resources 与正在运行的 Downloads EXE 解包 Core 的哈希和 manifest 一致；运行时 app.asar 与打包目录一致，四个 JS/CJS 主进程/预加载 bundle 和十二个 renderer 文件与对应构建逐字节一致。比较排除了打包规则不包含的 sourcemap。r2 打包使用 `--publish never`；r3 只更新独立插件，没有发布步骤。

旧候选保留，不建议继续使用或与本轮文件混用。

## 根因修复与回归依据

1. **正常搜索耗尽 fuel**：重复完整 JSON 解码/克隆和无关元数据消耗了 Wasm 预算。插件改为紧凑 typed parsing，宿主增加受限结构化 JSON 响应；生产检查覆盖第一页/第二页 20 条及 50 条，不是只取三条的接口探针。10,000,000 fuel、15 秒、64 MiB 限制均未提高。
2. **封面走错 QQ 接口或 renderer 联网**：按歌曲 Provider/Profile 使用宿主图片缓存，校验 PNG/JPEG/GIF/WebP、256 KiB 上限及 Origin/redirect；不放开 CSP。兼容已观察到的网易云 `image/jpg`。
3. **模式变化不刷新 GUI**：成功的插件操作缺少 `plugin://changed` 事件。真实 duplex 协议测试先复现安装后无事件，再验证修复；事件仅含插件 ID/启用标记。正常模式控件两向刷新及实际进程重启均检查。
4. **聚合播放丢失原生引用**：联合 ID 曾替换掉 `song.provider`，QQ MID/numeric ID/media ID 等不能恢复，音质也错用 catalog ID。路由现保留完整原生引用，并在解析/缓存/音质命令前恢复；旧路由从来源详情恢复，跨 Provider/Profile 的嵌入引用拒绝。测试覆盖四种来源、非默认 Profile、实际 resolver 参数和旧 ID。
5. **二维码和播放 UI 串平台**：外部二维码不再使用 QQ 提示；当前音质根据正在播放歌曲路由，不跟随当前目录。二维码分别观察 30 秒、取消和重开。
6. **无歌词空态不可读**：空态沿用浅色主题的黑色字，而歌词背景会变暗。仅两处颜色改为场景前景色；测试先失败于 `--text-tertiary`，修复后通过，真实 GUI 的浅色主题下空态已另行截图检查。
7. **已有兼容修复**：无 WASI 的 Component 编码、二维码 PNG fuel 优化、NetEase EAPI golden vector、KuGou detail/媒体签名/歌词匹配、宿主受限 OAuth 回调、账号未知权益和分页、平台中性错误。协议和安全边界见 [integration.md](integration.md)。
8. **搜索行播放丢失网易云封面**：上游搜索 `picUrl` 为 HTTP，详情另一次响应为 HTTPS，旧的详情检查漏掉了播放器栏。新增聚合播放器封面解码断言在 r2 真实失败；r3 在 raw/compact 搜索 mapper 中统一升级经官方 Origin 校验的地址，单元测试先红后绿，Core 探针另检查搜索封面 scheme。伪造后缀、userinfo、端口和 fragment 不升级；不放开 HTTP、CSP 或清单权限。检查为 O(URL 长度) 时间和空间。

## 针对性自动化

| 范围 | 结果 |
| --- | --- |
| 插件 Rust | 29/29 |
| Core plugin 模块，含五个 federation 测试 | 95/95 |
| Core stdio/插件事件协议 | 6/6 |
| Core player | 64/64 |
| 八个前端文件：App、catalog/search、artwork、native application、AccountDialog、PlayerBar、LyricsPanel | 170/170 |
| Desktop OAuth window / Host IPC | 67/67 |

这些不是全仓测试。生产前端 typecheck/build、release-bundle 检查、改动前端的 Prettier、两仓 `git diff --check` 通过。新增 federation/runtime/proto06 单独 rustfmt 通过；不声称整个历史 Core 的格式差异已清理。

Windows PowerShell 5.1 的完整插件 fmt/check/test/release build/Component/包检查已针对 r3 payload 通过。剩余历史编译/大 chunk 警告未顺手改动。

## Packaged Core 生产探针

证据：[host-smoke-search-cover-routing-fix-20261008-r3.json](../artifacts/host-smoke-search-cover-routing-fix-20261008-r3.json)。

探针启动临时隔离 Core，音量为零；每曲结束后暂停并清队列。三平台不同偏好、isolated/aggregate 两向变更和各自真实 Core 重启均验证。

| 样本/模式 | 播放时钟 | 当前音质切换后 | 观察 |
| --- | --- | --- | --- |
| 网易云屋顶 `5257138` / isolated | 1460 ms | 3005 ms | 55 行歌词、实际 high、26446 字节封面；搜索封面 HTTPS |
| 同录音 / aggregate | 1520 ms | 2975 ms | 原生引用恢复，high 继续播放；搜索封面 HTTPS |
| 酷狗 Monkeys `F09B7FAE3D1A3860C10738EBB33DCBBA` / isolated | 1455 ms | 2890 ms | standard；请求 high 后以 account-rights 明确降为 standard；搜索封面 HTTPS |
| 同录音 / aggregate | 1465 ms | 2960 ms | 聚合音质路由正常，同样明确降级；搜索封面 HTTPS |
| 酷狗晴天/两只老虎 / 匿名 | 不播放 | 不适用 | 上游权益拒绝；不是成功播放 |

已知可播样本若出现 `playbackError`、`qualitySwitchError`、缺少前进时钟或音质结果，脚本会非零退出；不再仅记录错误却让整次通过。匿名受限样本是拒绝路径，另列诊断。

Spotify 只验证缺少用户 Client ID 会拒绝、renderer 无权访问 host-only OAuth 准备，以及临时 fixture Client ID 的 S256/精确回调。没有打开真实授权 URL或代用户同意。该 Node 脚本仅是开发测试工具，不在插件/便携包内，不是运行依赖。

## 正常 GUI 控件

最终使用 Downloads 中的 r3 EXE 和当前插件，首次安装前确认测试 Profile 没有插件，三个外部账号均为 guest。安装通过 IPC 作测试准备，不将原生文件选择器称为已验收；其余功能输入是正常点击、键盘、滚动和选择控件。IPC snapshot 仅佐证结果。

隔离的是 Core 数据/插件和 Electron 用户目录，不是 Windows 系统凭据库。额外只读取账号状态确认内置 QQ 仍为 authenticated；没有提取、打印或清除凭据，也没有操作 QQ 登录/退出。QQ GUI 搜索/详情并非匿名测试。独立 Core 探针使用自己的文件凭据目录，与该 GUI 情况不同。

证据：`<YAQMC>/release-electron\search-cover-routing-fix-20261008-r3\gui-qa.json`，截图在同目录 `gui`；归档副本见 `artifacts/gui-qa-search-cover-routing-fix-20261008-r3.json`。检查清单见对应 `artifacts/gui-qa-search-cover-routing-fix-20261008-r3.md`，产物校验见 `artifacts/artifact-verification-search-cover-routing-fix-20261008-r3.json`。

- 安装后无需 renderer reload，isolated 四个平台；设置 aggregate 后五个；改回四个，没有不可用的残留。完整关闭/重新打开 Downloads EXE 后，两种模式仍各自正确。
- 两平台正常 20 条歌曲搜索，滚动后 40 条；详情封面解码并显示。联合结果含 QQ、网易云和酷狗，未配置 Spotify 不阻止其他来源。
- isolated 两平台普通播放、暂停后时钟稳定、恢复、seek、请求高品质与歌词页面。网易云 seek 到约 31.9 秒，酷狗约 12.5 秒；酷狗实际标准档不冒充高品质。
- aggregate 通过歌曲行播放正确的网易云/酷狗样本，明确断言播放器栏图片已解码，再用当前音质控件切换并打开封面详情；两个来源时钟均前进。不是把匿名 QQ 首条失败当成全部聚合失败，也不以详情封面代替搜索行播放封面检查。
- 正在播网易云时切换目录到酷狗，当前音质仍路由网易云，播放器封面保持正确。
- 两个平台二维码各六次五秒采样，保持 waiting-for-scan、同一可解码图片；取消后重开获得不同二维码，再取消。不含 QQ 专属说明。
- 空 Spotify Client ID 显示配置错误，账号保持 guest；不会假装进入 OAuth，也不再显示启动本地服务的要求。
- 匿名酷狗晴天显示 entitlement-insufficient 与当前平台权益提示，不计为可播。
- QQ 搜索/详情保持内置路由；没有声称 QQ 完整播放/账号回归通过。
- 探索检查含不支持的歌手搜索不返回歌曲、快速替换查询以最新为准，以及继续切换歌曲查询。

视觉检查单独打开截图，确认二维码没有遮挡/消失、提示未串平台、封面与歌词空态可见。默认 renderer 为 1280×802；另外以 1000×652 renderer 模拟检查搜索、详情、歌词和登录弹窗，侧栏/顶栏/播放器/弹窗均在视口内。小尺寸长标题正常省略。Electron 不支持本次 CDP 的原生 window-bounds 方法，所以不把模拟检查冒充真实 OS 缩放/窗口位置验收。

酷狗 Monkeys 样本的上游 400×400 JPEG 本身是渐变图，不能因看起来像占位图就断言图片请求失败；另保存并打开缓存 JPEG 核对，DOM 图片已完成解码。网易云使用真实专辑封面，两种模式的播放器和详情分别检查。

前一个测试窗口后来出现真实账号/操作状态，受干扰的匿名拒绝断言不纳入通过依据，也没有退出或修改该账号。r3 的匿名检查从新的隔离 Profile 重新完成。脚本对“已拒绝高品质不可再点击”、缺少 Client ID 时仍为 guest、进入动画尚未结束等错误假设作了修正，未通过 force-click 或更改产品行为绕过检查。

## 尚未验收

- 用户 Client ID 的真实 Spotify OAuth、可用试听及会话恢复。
- 真实网易云/酷狗扫码确认、恢复和退出登录的跨平台隔离。
- 真实账号库、歌单曲目、分页、无权限歌单和认证失效处理。
- 可听播放及地区/付费/无损/Hi-Res/母带资源；保存偏好和匿名 high 不证明会员资源可用。
- 内置 QQ 完整回归，以及原生安装文件选择器/OS 窗口缩放。
- 外部首页、发现、专辑/歌手/歌单目录尚未实现；需修复或由用户明确接受，不能称全功能完成。

按 [MANUAL-TEST.md](MANUAL-TEST.md) 人工验收。在上述门槛满足及用户明确批准前，不创建 Tag/Release。

## 复现 Core 探针

在插件仓库运行：

```powershell
node scripts/test-host-smoke.mjs '<YAQMC>/release-electron\search-cover-routing-fix-20261008-r3\win-unpacked\resources\core\yaqmc-core.exe' '<Downloads>/org.yaqmc.providers.music-0.2.0-search-cover-routing-fix-20261008-r3.yaqmc-plugin'
```

脚本仅在自己的临时 Profile 授权清单权限，使用子进程级 10808 代理，默认删除自己的临时目录；`--keep-profile` 可保留。不会读取/输出真实凭据或执行发布。
