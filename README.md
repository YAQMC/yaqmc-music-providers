# YAQMC Music Providers

为 [YAQMC](https://github.com/YAQMC/YAQMC) 接入 **Spotify、网易云音乐、酷狗音乐和汽水音乐**的音源插件，可与内置 QQ 音乐一起使用。

四个平台打包为一个 `.yaqmc-plugin` 文件，平台接口在 YAQMC 的 WebAssembly 沙箱内运行，无需单独启动后端或安装 Node.js。Spotify 桌面受保护播放的 HTML 和 hls.js 也随插件分发，由宿主在隔离播放窗口运行。当前插件版本为 **0.1.1-beta.1**，要求 **YAQMC 0.1.3-beta.4 或更新版本**，支持 Windows、Linux 和 Android。

## 功能

| 功能       | Spotify（Client ID 可选） | 网易云音乐               | 酷狗音乐                 | 汽水音乐                                 |
| ---------- | ------------------------- | ------------------------ | ------------------------ | ---------------------------------------- |
| 搜索       | 歌曲、歌单                | 歌曲、歌手、专辑、歌单   | 歌曲、歌手、专辑、歌单   | 已接入歌曲、歌单；平台目前拒绝（见下文） |
| 首页与发现 | 网页播放器首页、排行榜    | 支持                     | 新专辑、推荐歌单、排行榜 | 发现页推荐歌单                           |
| 公开歌单   | 打开并浏览曲目            | 打开并浏览曲目           | 打开并浏览曲目           | 打开并浏览曲目（分页）                   |
| 账号音乐库 | 喜欢的歌曲、个人歌单      | 喜欢的歌曲、个人歌单     | 喜欢的歌曲、个人歌单     | —（不支持登录）                    |
| 最近播放   | —                         | —                        | 账号听歌历史，只读分页   | —                                  |
| 歌词       | —                         | 行同步、译文、罗马音     | KRC 逐字同步，LRC 降级   | 逐字同步                           |
| 歌曲评论   | —                         | 热门、最新、楼中楼       | 热门、最新、楼中楼       | —                                  |
| 播放       | Web 账号：受保护播放实验（见限制）；Client ID：试听 | 按平台返回的可用资源播放 | 按平台返回的可用资源播放 | 公开网页播放器的未加密音频，常为约 30 秒试听 |

“—”表示当前插件未提供该功能。Spotify 留空 Client ID 时可登录网页版账号，也可未登录浏览公开目录，见“Spotify”一节。登录方式和播放限制见下文；QQ 音乐由 YAQMC 内置音源提供。

## 安装

需要使用支持 API v3 插件及本插件所需登录、凭据和评论接口的新版 YAQMC 宿主。Spotify 网页版账号登录需要宿主实现隔离浏览器的 `browserSession` Cookie 导入，以及 `protected-hls` 清单、分片和 Widevine 许可证代理；当前 Windows 与 Android 宿主均已实现。Linux 宿主暂未提供 DRM 播放后端，目录和账号功能仍可使用。酷狗目录接口需要宿主允许 `kg-tid` 协议头；账号音乐库需要宿主统一快照和分页的账号修订号；最近播放还需要宿主支持 `kugou-play-history` 只读签名、`provider-account` 历史来源和 `responseJsonFields` 响应字段过滤。汽水音乐同样依赖 `responseJsonFields`：其公开页面响应较大，未经宿主裁剪会超出插件沙箱的计算预算。缺少这些支持时，须同时更新宿主。安装时请确认所需的 `singerimg.kugou.com` 和 `c1.kgimg.com` 图片访问权限。

汽水音乐新增以下网络权限，升级时须重新确认：`beta-luna.douyin.com`（发现、歌单、单曲与歌词）、`api.qishui.com`（搜索）、`p3-luna.douyinpic.com` 和 `p6-luna.douyinpic.com`（封面），以及音频 CDN `v3-luna`、`v5-ex-luna`、`v11-luna`、`v26-luna`（均为 `.douyinvod.com`）。宿主只按精确来源授权，平台把音频分配到其他 CDN 节点时，该曲目会提示无法播放。

Spotify 网页账号模式需要 `open.spotify.com`、`api-partner.spotify.com`、`clienttoken.spotify.com`、`spclient.wg.spotify.com` 和 `mosaic.scdn.co` 权限：分别用于登录令牌、Pathfinder 目录、Web Player 设备令牌、受保护媒体与许可证以及部分复合歌单封面。音频 HLS 和分片还使用 `audio-ak`、`audio4-ak`、`audio-fa`、`audio4-fa`、`audio-cf`、`audio4-cf`、`audio-gc`、`audio4-gc` 对应的 Spotify CDN 主机；自定义歌单封面另需 `image-cdn-fa.spotifycdn.com` 和 `image-cdn-ak.spotifycdn.com`。从先前版本升级到此 beta 时，请确认这些新增来源和排行榜封面 `charts-images.scdn.co`。

本版本的联合音源与汽水音乐已在 YAQMC 0.1.3-beta.3（Linux 内核，临时配置）上测试；更早的宿主版本未经本次测试。

**通过插件商店**：在 YAQMC 的“插件 → 插件商店”中搜索本仓库（仓库带有 `yaqmc-plugin` 主题），选择发布中的 `.yaqmc-plugin` 安装包，下载后审核权限并安装。商店只读取 GitHub Release 附件，看不到 Actions 产物；本包是预发布版本，商店默认只展示稳定版时请手动下载。

**手动安装**：

1. 从 [Releases](https://github.com/YAQMC/yaqmc-music-providers/releases) 下载 `.yaqmc-plugin`；尚无 Release 时，打开 [Actions → CI](https://github.com/YAQMC/yaqmc-music-providers/actions/workflows/ci.yml)，选择 `main` 分支最近一次成功的运行，在 **Artifacts** 中下载 `music-provider-wasm` 并解压（需要登录 GitHub）。
2. 在 YAQMC 的“插件 → 插件管理”中选择“选择文件”安装该文件，查看并授予平台网络访问等所需权限。
3. 在插件详情的“配置”中选择工作模式，然后到账号页面登录需要使用的平台。Spotify 留空 Client ID 时使用网页版账号登录；填写 Client ID 时使用官方 Web API 与 PKCE。

也可以按照下方步骤从源码构建。插件安装包名为 `org.yaqmc.providers.music-0.1.1-beta.1.yaqmc-plugin`。

## 工作模式

在插件设置中切换，无需分别安装多个包。

| 模式               | 音源列表                                                                                         |
| ------------------ | ------------------------------------------------------------------------------------------------ |
| `isolated`（默认） | Spotify、网易云音乐、酷狗音乐、汽水音乐分别显示                                                  |
| `aggregate`        | 保留独立音源，另增加 `YAQMC All Music Platforms` 联合音源，聚合本插件的四个平台与 YAQMC 默认音源 |

联合音源保留歌曲来源，播放、音质和账号操作仍交给对应平台。各平台分别登录，账号凭据按平台和账号配置（Profile）隔离。未配置 Spotify 时，其他平台仍可使用。

### 联合搜索与歌单

切换到联合音源后，搜索页的“歌曲”和“歌单”标签会同时返回各平台结果，无需切换音源：

- 每条结果保留原平台、原始 ID 和账号配置。不同平台的相同 ID 是不同条目，插件不会按标题或歌手跨平台去重。
- 从结果中打开歌单，会由原平台读取曲目；歌词、详情和播放同样交给原平台，并显示原平台标签。
- 某个平台不支持该搜索类型、请求失败或会话过期时，只缺少该平台的结果，其他平台照常显示。Spotify 游客模式会返回公开歌曲和歌单；汽水音乐搜索目前会被平台拒绝（见“使用限制”）。
- 分页和排序由宿主决定：每一页向各平台请求同一页码，再轮流合并到本页条数上限（默认 20）。因此各平台每页只显示前若干条，其余结果不会出现在后续页中；结果按平台轮流排列，不按相关度统一排序。“加载更多”在任一平台还有结果时可用。
- 首页和发现页只使用联合音源的第一个成员（通常是 QQ 音乐）。

“我的歌单”与公开歌单搜索分开：当前宿主的联合音源不提供账号接口，无法把多个已登录平台的私人歌单合并到一个视图。请在各平台的独立音源中查看各自的账号歌单（只读）。公开歌单搜索不包含任何私人歌单。跨平台创建、写入、导入或同步歌单不在本插件范围内。

## 歌曲来源标签

本插件通过清单中的 `songBadges` 声明 QQ 音乐、网易云音乐、酷狗音乐、汽水音乐和 Spotify 的来源标签，包含简体中文、繁体中文、英文名称及颜色。需要支持 `ui.songBadges` 的新版 YAQMC；仅支持旧 API v3 的宿主仍需更新。升级插件时请确认新增权限。

独立模式按歌曲的 provider 匹配；联合音源按宿主保留的 `provider.originProviderId` 匹配，显示原平台。标签仅用于展示，不修改歌曲标题、ID 或播放路由。未知来源不显示标签；禁用、卸载插件或开启安全模式后标签消失，包括本插件为内置 QQ 音乐提供的标签。旧版本持久化的聚合歌曲若缺少原始 provider 信息，重新加载曲目后才能显示标签。

平台映射由本插件维护，宿主只提供通用展示接口。此功能不创建 JavaScript Worker、不请求额外网络资源，也不逐曲调用插件。

## 登录与配置

### 网易云音乐、酷狗音乐

在对应平台的账号页面选择 QQ、微信网页登录，或使用平台 App 扫码。

QQ、微信入口仍受平台验证规则限制；授权完成后，平台可能继续要求用自己的 App 扫码。酷狗第三方登录遇到 `20002` 提示时，请改用酷狗 App 扫码。

### 汽水音乐

无需配置，也不支持登录：插件只使用平台公开网页接口，不保存任何汽水音乐凭据。从汽水音乐的发现页打开推荐歌单，即可浏览曲目、查看逐字歌词并播放平台公开返回的音频。

插件不向宿主报告任何登录方式，账号状态始终为访客且不声明任何账号能力。当前 YAQMC 宿主仍会在设置页为所有支持账号接口的音源显示“登录”按钮和“通过官方页面登录”的说明；点击后对话框只能提示“当前没有可用的登录方式”。这是宿主界面的限制，需要宿主改为在音源未报告任何登录方式时隐藏该按钮。

扫码登录未实现：参考实现使用抖音 App 扫码的 Passport 流程，但本插件没有可用的测试账号来验证扫码确认、过期和凭据保存，因此不提供未经验证的登录入口。

### Spotify

插件按去除首尾空白后的 **Spotify Client ID** 设置选择连接方式：

| Client ID     | 连接方式                          | 结果                                                            |
| ------------- | --------------------------------- | --------------------------------------------------------------- |
| 已填写        | 官方 Web API + OAuth PKCE         | 歌曲、歌单搜索，账号音乐库，试听                                |
| 留空或仅空白 | 网页版账号登录 + 网页播放器目录   | 无需 Client ID 即可登录、读取音乐库；受保护播放还取决于许可证和运行时能力 |

留空 Client ID 时，账号页会显示 **Spotify Web Account**。登录窗口打开 Spotify 自己的登录页；宿主从隔离会话中导入 `sp_dc`、`sp_key`、`sp_t` Cookie，插件用这些凭据向网页播放器的 `/api/token` 换取账号访问令牌，再通过 Pathfinder 读取账号资料、喜欢的歌曲、账号歌单、歌曲和歌单搜索。Cookie 与令牌只保存在 YAQMC 的凭据存储中，应用重启后可以恢复；退出账号会清除两者。

未登录时仍可浏览公开目录：插件取得匿名网页令牌和服务器时间后，读取公开歌曲、歌单及歌单曲目。匿名令牌是短时会话，不写入插件设置，也不会用于账号音乐库。

首页读取网页播放器推荐的歌单与专辑，发现页读取排行榜；点击专辑可浏览其曲目。账号歌单列表会按歌单修订号缓存曲目总数。播放时，插件先向 Web Player 的 `/track-playback/v1/media` 取得加密 HLS 清单和签名分片，并声明许可证、证书请求和 `browserPlayer` 资源。桌面端的 HLS/EME 播放逻辑位于插件的 `assets/spotify-player.html` 与 `assets/hls.min.js`；宿主提供隔离窗口、平台 CDM/解码器、本机媒体代理、播放命令和状态通道。更新播放脚本只需更新插件，资源缺失则明确失败，不自动切换播放方式。Android 使用宿主的系统媒体/DRM 原语执行插件的请求描述。音频不会经过插件 Wasm 沙箱，签名 URL 和凭据也不会交给播放页面。Web Player 对自定义 Client ID 模式不可用，因此填写 Client ID 时仍使用官方 Web API 返回的试听地址。

Windows Electron 宿主的 DRM 运行时还需要服务器接受的 VMP 签名。CDM 初始化成功、清单可读取和账号已登录，都不代表许可证交换已经成功。使用 Castlabs Electron 的宿主维护者需在修改可执行文件后完成生产 VMP 签名；普通用户只需登录 Spotify，不需要 EVS 账号。许可证返回相对地址时，插件按 Web Player 的 webgate 来源解析并绑定同来源凭据；缺失或不支持的地址会明确报错，不猜测其他许可证端点。

网页账号与 OAuth 是两套独立连接方式。填写 Client ID 后，插件改用官方 Web API 和 PKCE，不会使用之前保存的网页 Cookie；清空 Client ID 后，插件改回网页账号，不会使用保存的 OAuth 令牌。两种登录状态都会保留，退出账号会一起清除。

Spotify 可能轮换网页播放器的验证参数、Pathfinder 查询哈希或 Cookie 规则。届时插件会返回需要重新登录或可重试的离线错误，不会伪造成功结果。

歌手详情通过 Spotify 公开的 oEmbed 接口读取名称和头像，无需登录或设置 Client ID。该接口不提供歌曲或专辑列表。

使用官方 Web API：

1. 在 [Spotify Developer Dashboard](https://developer.spotify.com/dashboard) 创建应用。
2. 在应用设置中登记以下回调地址，须完全一致：

   ```text
   http://127.0.0.1:43821/oauth/spotify/callback
   ```

3. 将 **Client ID** 填入 YAQMC 的插件设置，再从账号页面发起 Spotify 登录。

插件使用 OAuth 2.0 PKCE，不需要 Client Secret。Token 和 Cookie 由 YAQMC 的凭据存储管理，插件包不包含预置账号。

## 使用限制

- **Spotify 整曲播放尚未通过验收。** Client ID 留空时可使用网页账号登录、搜索和账号音乐库；插件提供 HLS/EME 播放页及许可证请求描述，宿主提供隔离窗口、媒体代理和平台 DRM 原语。Windows 实测清单及证书返回 200，许可证返回 403；打包运行时的 VMP 状态为 `PLATFORM_TAMPERED`，本次未执行生产 VMP 签名。Android 的最新受保护播放实现尚未经本次真机验证，Linux 未提供 DRM 播放后端。Client ID 模式仍只使用官方 Web API 提供的试听。此 beta 不承诺 Spotify 整曲可播放。
- **网易云、酷狗的播放和音质取决于平台返回结果。** 会员、地区、版权及接口限制可能使歌曲不可播放或只能使用较低音质。
- **酷狗会员曲目可降级为官方试听。** 完整播放被平台明确拒绝，且平台提供独立试听资源时，插件会尝试播放不超过 90 秒的片段，并向 YAQMC 标记为试听；没有可用片段时仍返回限制原因。
- **歌词动画使用平台的真实时间数据。** 酷狗优先读取 KRC 逐字歌词，仅有 LRC 时使用行同步；网易云 LRC 保留译文和罗马音。行同步不伪造逐字时间。
- **网易云、酷狗评论为只读。** 支持热门/最新分页及楼中楼，展示作者、头像、日期和点赞数，不提供发评或点赞。歌词弹幕与评论面板共享宿主的热门评论缓存。
- **网易云楼中楼存在读取上限。** 回复按完整线程的时间倒序分页，最多读取 500 条；无法完整读取或超出上限时明确报错，不返回局部倒序结果。酷狗使用原生最热、最新回复接口分页，不受这一完整线程排序上限限制。
- **酷狗音乐库为只读。** 支持读取“喜欢的歌曲”、个人歌单和歌单内歌曲，不提供收藏或歌单写操作。头像使用酷狗账号资料，可能与第三方登录账号的头像不同。
- **酷狗最近播放使用账号历史。** 保留平台播放时间，按小页加载；当前批次在本地缓存 5 分钟，最多保留 4 个批次，缓存过期后需重新加载。不会上传或修改平台听歌记录。
- **汽水音乐搜索目前不可用。** 平台搜索接口拒绝未签名的请求（返回空结果），所需签名属于设备校验，插件不会伪造，因此搜索会提示错误，联合搜索中也没有汽水音乐结果。请从汽水音乐发现页的推荐歌单进入。
- **汽水音乐只播放公开网页播放器返回的音频。** 平台公开提供完整音频的曲目可完整播放；会员曲目通常只返回约 30 秒的未加密试听，会标记为试听。需要 PlayAuth 解密的音频不被支持；歌单中缺少可用音源的曲目显示为不可播放。
- 平台接口或登录规则变化可能影响功能。出现问题时，请提供平台、操作步骤和错误提示，并隐藏 Cookie、Token 等账号信息。

## 从源码构建

需要 [Rust / rustup](https://www.rust-lang.org/tools/install) 和 [PowerShell 7](https://github.com/PowerShell/PowerShell)。仓库通过 `rust-toolchain.toml` 固定 Rust 1.88.0，构建目标为 `wasm32-unknown-unknown`。

在仓库根目录执行：

```powershell
rustup target add wasm32-unknown-unknown
pwsh ./scripts/build.ps1
```

生成的插件包位于：

```text
dist/org.yaqmc.providers.music-0.1.1-beta.1.yaqmc-plugin
```

构建后验证包内容：

```powershell
pwsh ./scripts/validate.ps1 -SkipBuild
```

运行 Rust 测试：

```powershell
cargo test --workspace --locked
cargo test --manifest-path plugins/music/Cargo.toml --locked
```

网页播放器状态回归使用 Node.js 26.7.0 的内置测试工具，不需安装 npm 依赖：

```powershell
node --test scripts/spotify-player.test.mjs
```

安装到临时宿主配置中检查汽水音乐、Spotify 网页账号准备、游客目录和联合搜索/歌单路由（需要已构建且支持 `browserSession` 的 YAQMC Core，会访问真实平台接口；调试版宿主较慢时可设置 `YAQMC_SMOKE_TIMEOUT_MS`）：

```bash
node scripts/test-host-smoke.mjs <yaqmc-core> dist/org.yaqmc.providers.music-0.1.1-beta.1.yaqmc-plugin --federation
```

CI 会在推送和 Pull Request 时执行测试、格式检查、Clippy、构建及包验证，并将插件包上传到 Actions 的 `music-provider-wasm` 产物中。

## 代码布局

| 路径                                        | 用途                           |
| ------------------------------------------- | ------------------------------ |
| `plugins/music/`                            | 当前音乐插件的 Rust 实现与清单 |
| `wit/yaqmc-provider/`                       | 插件与宿主的接口定义           |
| `tools/componentize/`                       | WebAssembly Component 打包工具 |
| `scripts/build.ps1`、`scripts/validate.ps1` | 构建与包验证                   |
| `.github/workflows/ci.yml`                  | CI 与产物上传                  |

`crates/backend/`、`crates/platforms/`、`runtime/` 以及旧的独立平台插件目录保留早期实现，不参与当前 `plugins/music` 插件的运行。`docs/configuration.md` 和 `docs/integration.md` 描述的也是早期后端方案；当前插件的安装与构建以本页为准。

## 许可证

Rust 实现使用 [MIT](LICENSE)；插件中的网页播放器使用 GPL-3.0-or-later，hls.js 使用 Apache-2.0，对应许可证随 `assets/` 一同打包。第三方协议参考及相关声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

## 发布（维护者）

`.github/workflows/release.yml` 只在推送 `v*` 标签或手动触发时运行，不会随普通推送发布。它要求标签与 `plugins/music/manifest.json` 的版本一致（例如 `v0.1.0`），重新执行测试、构建、`validate.ps1` 和 `test-packaging.ps1`，然后把 `.yaqmc-plugin` 与 `SHA256SUMS.txt` 上传为 GitHub Release 附件；手动触发默认创建草稿。版本号含 `-` 时标记为预发布。YAQMC 插件商店按未验证来源处理这些附件：GitHub 托管和摘要不等于发布者签名。
