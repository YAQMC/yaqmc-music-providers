# YAQMC Music Providers

YAQMC 的独立音乐平台 Provider Component 仓库。它把 Spotify、网易云音乐和酷狗音乐接入 YAQMC，同时保留 YAQMC 内置的 QQ 音乐 Provider。

本仓库包含两部分：

- `Wasm Provider Component`：安装到 YAQMC 的插件包。
- `native backend`：独立运行的本地服务，负责上游 API、登录状态和媒体地址代理。

插件不会启动 Backend，也不会代理或读取 QQ 音乐 Cookie。使用前必须先启动 Backend。

## 当前状态

这是一个可构建、可安装的实验性集成。功能是否可用取决于平台账号、地区、上游 API 服务和播放权限。

| 平台 | 独立模式 | 登录方式 | 真实上游播放 |
| --- | --- | --- | --- |
| Spotify | 已实现 | 用户提供 Client ID，浏览器 OAuth 2.0 + PKCE | 需要可用播放会话和 Premium；Web API 搜索不保证完整音频 |
| 网易云音乐 | 已实现 | 兼容 API 提供二维码登录 | 取决于兼容 API 和账号状态 |
| 酷狗音乐 | 已实现 | 兼容 API 提供二维码登录 | 取决于兼容 API 和账号状态 |
| QQ 音乐 | YAQMC 内置 | 由 YAQMC 管理 | 不经过本仓库 Backend |

## 两种模式

模式在构建插件包时确定，不是 YAQMC 运行时开关。

### 独立模式

每个平台对应一个 YAQMC Provider。可以同时安装并启用三个包：

- `org.yaqmc.providers.spotify`
- `org.yaqmc.providers.netease`
- `org.yaqmc.providers.kugou`

这是需要分别登录、分别管理账号时的推荐模式。登录状态按 `(platform, profileId)` 隔离，退出一个平台不会删除其他平台的状态。

### 聚合模式

一个 Provider 的搜索和收藏库请求会并发请求已配置的外部平台，并在结果 ID 中保留来源，例如：

```text
netease::163000001
```

聚合结果中的来源由 `platform`、`profileId` 和 `trackId` 组成，歌词、歌曲详情和播放解析会按这个来源回到正确的平台。

三个聚合包使用同一套 Backend 扇出逻辑，实际安装时只选择其中一个，否则结果会重复。当前 YAQMC 的账号界面是单 Provider 账号模型，因此需要分别登录和管理多个平台时使用独立模式；聚合模式主要用于统一搜索和库浏览。

## 工作方式

```text
YAQMC
  │ Provider Component / Wasm
  │ POST http://127.0.0.1:43821/v1
  ▼
本仓库 Backend
  ├─ Spotify Web API / OAuth
  ├─ 网易云兼容 API
  └─ 酷狗兼容 API
```

Backend 只监听本机回环地址。它把 OAuth Token、平台 Cookie 和短期媒体代理状态保存到 `dataDir`，Wasm 插件本身不保存这些凭据。

## 前置条件

- Windows 或 Linux
- Rust `1.88` 或更新版本
- `wasm32-wasip2` 编译目标
- 支持 Provider Component API v3 的 YAQMC Desktop

```powershell
rustup target add wasm32-wasip2
```

### YAQMC 宿主要求

插件清单申请的是精确权限：

```text
network:http://127.0.0.1:43821
```

不能改成 `localhost`、其他端口或网络通配符。YAQMC 宿主必须允许这个精确的本地 HTTP origin；如果你的 YAQMC 版本拒绝该权限，需要先升级或应用宿主侧的最小回环权限支持。本仓库不把 YAQMC Core 源码复制进来，也不负责自动修改 YAQMC。

## 快速开始

以下命令在本仓库根目录执行。

### 1. 生成配置

```powershell
cargo run -p yaqmc-music-providers-backend -- init --path config.local.toml
```

编辑 `config.local.toml`。最小配置示例：

```toml
dataDir = "data"
mode = "isolated"

[listen]
host = "127.0.0.1"
port = 43821

[spotify]
clientId = "填入你自己的 Spotify Client ID"
redirectUri = "http://127.0.0.1:43821/oauth/spotify/callback"

[netease]
baseUrl = "https://你的网易云兼容 API 地址"

[kugou]
baseUrl = "https://你的酷狗兼容 API 地址"
```

`mode` 是 Backend 的部署提示；真正决定插件行为的是下面的 `scripts/build.ps1 -Mode`。

### 2. 配置 Spotify Client ID

Client ID 必须由使用者提供。本仓库不包含 Client ID、Client Secret、Access Token、Refresh Token 或测试账号。

可以写入 TOML，也可以使用环境变量；环境变量优先：

```powershell
$env:SPOTIFY_CLIENT_ID = "你的 Spotify Client ID"
```

在 Spotify Developer Dashboard 中登记完全一致的回调地址：

```text
http://127.0.0.1:43821/oauth/spotify/callback
```

当前流程使用 PKCE，不需要 Client Secret。没有 Client ID 时，Spotify Provider 可以安装和注册，但账号登录及依赖 Spotify 配置的请求会返回 `provider-not-configured`。

### 3. 配置网易云和酷狗 API

本仓库只实现客户端适配器，不捆绑或启动网易云、酷狗 API 服务。`baseUrl` 必须指向兼容本项目请求和响应格式的 HTTP JSON 服务；不能把普通网页地址当成 API 服务。

也可以用环境变量覆盖：

```powershell
$env:NETEASE_API_BASE_URL = "https://你的网易云兼容 API 地址"
$env:KUGOU_API_BASE_URL = "https://你的酷狗兼容 API 地址"
```

生产环境使用 HTTPS。HTTP 只用于本机开发 Fixture。

### 4. 启动 Backend

Backend 必须保持运行：

```powershell
cargo run -p yaqmc-music-providers-backend -- serve --config config.local.toml
```

不要把监听地址改成公网地址。`dataDir/profiles.json` 包含登录凭据，应当当作密钥文件保护，也不应提交到 Git。

### 5. 构建插件包

独立模式：

```powershell
pwsh ./scripts/build.ps1 -Mode isolated
```

输出在 `dist/isolated/`：

```text
org.yaqmc.providers.spotify-0.1.0-isolated.yaqmc-plugin
org.yaqmc.providers.netease-0.1.0-isolated.yaqmc-plugin
org.yaqmc.providers.kugou-0.1.0-isolated.yaqmc-plugin
```

聚合模式：

```powershell
pwsh ./scripts/build.ps1 -Mode aggregate
```

输出在 `dist/aggregate/`。只安装其中一个聚合包。

### 6. 在 YAQMC 中安装

打开 YAQMC 的 **设置 → 插件 → 从文件安装**，选择对应的 `.yaqmc-plugin`。在权限审查页批准清单中的全部权限。

所有包都需要：

```text
provider.catalog
provider.playback
provider.lyrics
provider.account
plugin.storage
network:http://127.0.0.1:43821
```

Spotify 包还需要：

```text
network:https://accounts.spotify.com
```

启用后，在 YAQMC 的 Provider 列表中选择对应平台。安装或启用后如果出现 `network origin is not granted`，检查是否批准了精确的回环权限，并确认 Backend 端口仍为 `43821`。

## 登录和播放边界

- Spotify：通过浏览器 OAuth 完成登录；需要用户自己的 Client ID。登录不等于获得完整音频播放权限。
- 网易云音乐：兼容 API 支持时，在 YAQMC 中使用二维码登录；二维码轮询、刷新和取消由 Backend 管理。
- 酷狗音乐：兼容 API 支持时使用二维码登录。
- 需要登录的收藏库、歌单和受限播放请求，必须先为当前 `profileId` 完成登录；公开搜索不一定需要登录。
- `full-playback-unavailable` 表示上游没有返回可验证的完整音频源，不会把预览地址伪装成完整歌曲。

## 真实 YAQMC 验证记录

验证日期：2026-10-02。本仓库不仅运行 Rust 单元测试，还使用真实 YAQMC Electron + 生产 Core，在独立 QA 数据目录中通过 YAQMC IPC 安装和调用插件：

- 独立模式：Spotify、网易云、酷狗三个包均能安装、启用并注册；Provider 列表同时保留 QQ Music。
- 网易云：通过本地兼容 API Fixture 验证搜索、歌曲详情、歌词、Guest 账号状态，以及二维码登录的启动和取消。
- 聚合模式：通过真实 YAQMC 验证跨平台搜索，并验证来源 ID `netease::163000001` 可以继续请求歌词和歌曲详情。
- QQ Music：验证内置 Provider 与外部 Provider 共存；QQ 上游搜索可能受外部限流影响，不能把限流误判为插件安装失败。

上述实时功能验证使用的是本地 Fixture，不代表已经验证了你的 Spotify Client ID、网易云账号、酷狗账号或生产 API。Spotify 真实 OAuth、酷狗生产 API 和完整音频播放需要用户提供凭据或兼容服务后再测。

## 本地验证

```powershell
cargo test --workspace --locked
cargo test --manifest-path plugins/guest-core/Cargo.toml --locked
pwsh ./scripts/validate.ps1
```

`validate.ps1` 会构建并检查独立模式和聚合模式的全部插件包，确认包内只有合法的 Wasm Component、清单权限和入口文件正确。

## 常见错误

| 错误 | 处理 |
| --- | --- |
| `provider-not-configured` | 配置对应平台的 Client ID 或 `baseUrl`，然后重启 Backend |
| `authentication-required` | 为当前 Provider/Profile 完成登录 |
| `network origin is not granted` | 批准精确的 `network:http://127.0.0.1:43821` 权限 |
| `full-playback-unavailable` | 检查上游是否提供带长度和音频 MIME 的完整音频源 |
| Backend 无响应 | 确认 `serve` 进程仍在运行，且没有被防火墙或其他进程占用 `43821` |

## 目录

```text
crates/protocol/       Backend 与 Wasm 之间的版本化协议
crates/platforms/      Spotify、网易云、酷狗适配器
crates/backend/        本地 HTTP Backend、登录状态和媒体代理
plugins/*/             YAQMC Provider Component
wit/                   YAQMC Provider ABI
scripts/               构建和包校验脚本
docs/                  配置与集成细节
```

更多实现细节见 [`docs/configuration.md`](docs/configuration.md)、[`docs/integration.md`](docs/integration.md) 和 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。
