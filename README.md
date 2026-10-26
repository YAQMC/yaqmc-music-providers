# YAQMC Music Providers

一个自包含的 YAQMC Provider 插件，同时接入 Spotify、网易云音乐和酷狗音乐，并与 YAQMC 内置 QQ 音乐 Provider 共存。

当前版本为 `0.2.2` 人工验收候选版。人工扫码、Spotify OAuth 和播放验收通过前，不创建 Tag 或 Release。

## 关键特性

- 只安装一个 `.yaqmc-plugin` 文件。
- 不需要 Backend、Node.js、本地 API 服务或常驻进程。
- 插件通过 YAQMC 受控网络接口直接访问平台 HTTPS API。
- Token 和 Cookie 由 YAQMC 凭据存储保管；插件持有不可读句柄。
- Spotify Client ID 由用户在插件设置中提供，使用 OAuth 2.0 Authorization Code + PKCE，不保存 Client Secret。
- 网易云和酷狗可在独立小浏览器中通过 QQ、微信登录，也保留平台二维码入口。
- 歌曲封面按来源平台与 Profile 获取；只显示宿主校验后的图片，不放开网页任意联网。

## 平台能力

| 平台 | 登录 | 歌曲搜索/歌曲详情/歌词 | 播放边界 |
| --- | --- | --- | --- |
| Spotify | 用户 Client ID + PKCE | Web API 搜索/详情；不提供歌词 | 只使用 Spotify 返回的试听地址，不伪装成完整音频 |
| 网易云音乐 | QQ/微信网页登录、平台二维码 | 直连网易云接口 | 受账号、地区、版权和上游接口限制 |
| 酷狗音乐 | QQ/微信网页登录、平台二维码 | 直连酷狗接口 | 受账号、地区、版权和上游接口限制 |
| QQ 音乐 | YAQMC 内置流程 | YAQMC 内置 Provider | 不经过本插件 |

## 两种模式

模式在插件设置中切换，不需要重新打包。

### `isolated`

同时注册三个独立 Provider：

- Spotify
- NetEase Cloud Music
- KuGou Music

每个平台独立登录、独立存储凭据。QQ Music 继续由 YAQMC 注册。

### `aggregate`

保留上述三个独立 Provider，并额外注册 `YAQMC All Music Platforms` 联合 Provider。联合 Provider 聚合三个外部平台和 YAQMC 默认 Provider；歌曲保留来源 Provider、Profile 和完整原生曲目引用，播放与音质切换仍交给原平台，不串用账号或媒体 ID。未配置的 Spotify 不阻止其他平台返回歌曲。

切换模式后，音乐服务选项会异步刷新，不需要重启；两种模式也会在重启后保留。联合 Provider 没有另一个统一登录账号，登录仍在各平台完成。

## 安装

1. 在 YAQMC 插件页面安装 `org.yaqmc.providers.music-0.2.2.yaqmc-plugin`。
2. 审核并授权清单中的 Provider、存储和平台 HTTPS 域名权限。
3. 在插件设置中选择 `isolated` 或 `aggregate`。
4. 如需 Spotify，在插件设置中填写自己的 `Spotify Client ID`。
5. 在账号页面分别完成平台登录。

插件包不包含账号、Client ID、Client Secret、Cookie、Token 或密码。

当前插件 `0.2.2` 需要支持 `provider.comments` 的新版 YAQMC 宿主。网易云评论独立于歌词加载，支持热门/最新分页、头像、日期和楼中楼。回复线程完整读取后按时间倒序分页，完整排序最多读取 500 条；超过上限明确返回 `comments-limit-exceeded`。游标绑定歌曲、排序、父评论、账号配置及页大小，旧游标在升级后需要重新打开评论面板。

登录、音源、音质和版权由各平台实际响应决定；登录成功不代表会员权益或完整歌曲播放已获授权。插件不会绕过付费或版权限制。审计及当前验证范围见 [测试记录](docs/TEST-RESULTS.md)。

## Spotify 配置

在 [Spotify Developer Dashboard](https://developer.spotify.com/dashboard) 创建 Web API 应用，并登记完全一致的 Redirect URI：

```text
http://127.0.0.1:43821/oauth/spotify/callback
```

然后把应用的 Client ID 填入 YAQMC 插件设置。不要把 Client Secret 写入插件配置。

Spotify 官方支持桌面/客户端应用使用 PKCE。Lyricify 4 的公开仓库仅提供自定义 Spotify API Client 配置文档，没有公开授权实现源码；本插件参考其“用户自备应用与精确回调地址”的使用方式，授权协议以 Spotify 官方 PKCE 流程为准。

## 构建

要求：

- Windows PowerShell 5.1 或 PowerShell 7
- Rust 1.88
- `wasm32-unknown-unknown` 目标

```powershell
pwsh ./scripts/build.ps1
```

输出：

```text
dist/org.yaqmc.providers.music-0.2.2.yaqmc-plugin
```

构建流程先生成无 WASI 的 Core Wasm，再由仓库内 `yaqmc-componentize` 工具编码为 WebAssembly Component。YAQMC 沙箱不会向插件开放 WASI 环境、文件系统或进程能力。

## 验证

```powershell
cargo test --manifest-path plugins/music/Cargo.toml --locked
pwsh ./scripts/validate.ps1
```

`validate.ps1` 会检查：

- 插件版本和 API v3 清单；
- 三个 Provider 与联合 Provider 声明；
- 包内只有一个 WebAssembly Component；
- Component 不导入 WASI；
- 包内没有 Backend、Node.js 或原生可执行文件。

已完成的自动化与宿主级验证见 [`docs/TEST-RESULTS.md`](docs/TEST-RESULTS.md)。人工验收步骤见 [`docs/MANUAL-TEST.md`](docs/MANUAL-TEST.md)。

## 安全边界

- 网络请求受清单 HTTPS Origin 限制；网易云媒体根域授权还覆盖其官方媒体子域，酷狗媒体 CDN 按精确域名授权。
- 酷狗登录后的请求由 YAQMC 在宿主内注入 Token、用户 ID 并重新签名；插件不能通过签名接口读取已保存的凭据。
- Spotify Token、网易云 Cookie、酷狗 Token 不写入插件 KV 或日志。
- OAuth `state`、PKCE verifier、二维码 attempt/lease 均按 Profile 隔离。
- 登录取消、过期和插件卸载会清理短期状态；删除插件数据时由 YAQMC 删除凭据句柄。
- 插件不读取或修改 QQ 音乐凭据。

## 已知限制

- Spotify Web API 不提供完整歌曲音频；本插件只暴露可验证的试听资源。
- KuGou 收藏歌曲接口没有稳定可用的公开兼容协议；本插件不伪造收藏能力，`account.favorite-songs` 对 KuGou 返回 unsupported。
- Spotify `playlist-read-private` 只授权读取本人或协作歌单；无权限歌单应返回授权拒绝，不能被解释成登录状态失效。
- 平台私有接口可能变化，出现 `malformed-response` 时需要重新核对上游协议。
- 自动化测试只能验证 OAuth 准备、生产二维码创建和未扫码轮询，不能替代真实账号人工授权。
- 网易云支持歌曲/歌手/专辑/歌单搜索、首页、发现和相关目录；Spotify、酷狗仍有目录能力限制，未支持的操作明确返回错误。
- 音质偏好按平台和 Profile 保存，支持当前歌曲切换；请求档位与实际资源档位分别显示。`automatic` 当前请求标准档，不代表自动选择最高会员档位。
- Spotify 仅接受自动/标准试听档；酷狗不支持母带档。其他高音质是否可用由上游资源、版权与账号权益决定，不能保证无损、Hi-Res 或母带播放。登录成功不等于已确认会员权益。
- 酷狗高音质被上游明确拒绝时，最多尝试四档已支持音质并记录降级原因；网络错误不会触发盲目降档，也不绕过付费限制。
- 酷狗歌词候选须与歌曲标题匹配且时长差不超过两秒；无法核对时返回无歌词。保守筛选可能漏掉有效歌词，但不会直接采用明显不符的首条候选。
- `crates/backend`、`crates/platforms` 和 `runtime` 是早期 sidecar 方案的历史代码，不会进入当前插件包，也不是运行依赖。

## 参考实现

- Spotify PKCE：Spotify Web API 官方文档
- Spotify 自定义应用配置：`WXRIW/Lyricify-App` 的 Lyricify 4 Custom Client 文档
- 网易云二维码协议：`NeteaseCloudMusicApiEnhanced/api-enhanced`
- 酷狗二维码、签名与目录协议：`MakcRe/KuGouMusicApi`、`chthollyphile/folia-major`

引用和改写范围见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。
