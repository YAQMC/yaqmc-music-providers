# YAQMC Music Providers

为 [YAQMC](https://github.com/YAQMC/YAQMC) 接入 **Spotify、网易云音乐和酷狗音乐**的音源插件，可与内置 QQ 音乐一起使用。

三个平台打包为一个 `.yaqmc-plugin` 文件，在 YAQMC 的 WebAssembly 沙箱内运行，无需单独启动后端或安装 Node.js。当前插件版本为 **0.2.5**，支持 Windows 和 Linux。

## 功能

| 功能       | Spotify              | 网易云音乐               | 酷狗音乐                 |
| ---------- | -------------------- | ------------------------ | ------------------------ |
| 搜索       | 歌曲                 | 歌曲、歌手、专辑、歌单   | 歌曲、歌手、专辑、歌单   |
| 首页与发现 | —                    | 支持                     | 新专辑、推荐歌单、排行榜 |
| 账号音乐库 | 喜欢的歌曲、个人歌单 | 喜欢的歌曲、个人歌单     | 喜欢的歌曲、个人歌单     |
| 歌词       | —                    | 支持                     | 支持                     |
| 歌曲评论   | —                    | 热门、最新、楼中楼       | 热门、最新、楼中楼       |
| 播放       | 有试听地址时可试听   | 按平台返回的可用资源播放 | 按平台返回的可用资源播放 |

“—”表示当前插件未提供该功能。登录方式和播放限制见下文；QQ 音乐由 YAQMC 内置音源提供。

## 安装

需要使用支持 API v3 插件及本插件所需登录、凭据和评论接口的新版 YAQMC 宿主。酷狗目录接口还需要宿主允许 `kg-tid` 协议头；旧宿主拒绝该请求时，须同时更新宿主。安装时请确认所需的 `singerimg.kugou.com` 和 `c1.kgimg.com` 图片访问权限。

1. 打开仓库的 [Actions → CI](https://github.com/YAQMC/yaqmc-music-providers/actions/workflows/ci.yml)，选择 `main` 分支最近一次成功的运行。
2. 在 **Artifacts** 中下载 `music-provider-wasm`，解压得到 `.yaqmc-plugin` 文件。下载 Actions 产物需要登录 GitHub。
3. 在 YAQMC 的插件页面安装该文件，查看并授予平台网络访问等所需权限。
4. 在插件设置中选择工作模式，然后到账号页面登录需要使用的平台。Spotify 需先填写自己的 Client ID。

也可以按照下方步骤从源码构建。插件安装包名为 `org.yaqmc.providers.music-0.2.5.yaqmc-plugin`。

## 工作模式

在插件设置中切换，无需分别安装多个包。

| 模式               | 音源列表                                                                                         |
| ------------------ | ------------------------------------------------------------------------------------------------ |
| `isolated`（默认） | Spotify、网易云音乐、酷狗音乐分别显示                                                            |
| `aggregate`        | 保留独立音源，另增加 `YAQMC All Music Platforms` 联合音源，聚合本插件的三个平台与 YAQMC 默认音源 |

联合音源保留歌曲来源，播放、音质和账号操作仍交给对应平台。各平台分别登录，账号凭据按平台和账号配置（Profile）隔离。未配置 Spotify 时，其他平台仍可使用。

## 登录与配置

### 网易云音乐、酷狗音乐

在对应平台的账号页面选择 QQ、微信网页登录，或使用平台 App 扫码。

QQ、微信入口仍受平台验证规则限制；授权完成后，平台可能继续要求用自己的 App 扫码。酷狗第三方登录遇到 `20002` 提示时，请改用酷狗 App 扫码。

### Spotify

1. 在 [Spotify Developer Dashboard](https://developer.spotify.com/dashboard) 创建应用。
2. 在应用设置中登记以下回调地址，须完全一致：

   ```text
   http://127.0.0.1:43821/oauth/spotify/callback
   ```

3. 将 **Client ID** 填入 YAQMC 的插件设置，再从账号页面发起 Spotify 登录。

插件使用 OAuth 2.0 PKCE，不需要 Client Secret。Token 和 Cookie 由 YAQMC 的凭据存储管理，插件包不包含预置账号或密钥。

## 使用限制

- **Spotify 仅支持试听。** 只有上游返回试听地址时才能播放；没有试听地址的歌曲不可播放。登录或订阅 Spotify Premium 不会让本插件获得完整音频。
- **网易云、酷狗的播放和音质取决于平台返回结果。** 会员、地区、版权及接口限制可能使歌曲不可播放或只能使用较低音质。
- **酷狗会员曲目可降级为官方试听。** 完整播放被平台明确拒绝，且平台提供独立试听资源时，插件会尝试播放不超过 90 秒的片段，并向 YAQMC 标记为试听；没有可用片段时仍返回限制原因。
- **网易云、酷狗评论为只读。** 支持热门/最新分页及楼中楼，展示作者、头像、日期和点赞数，不提供发评或点赞。歌词弹幕与评论面板共享宿主的热门评论缓存。
- **网易云楼中楼存在读取上限。** 回复按完整线程的时间倒序分页，最多读取 500 条；无法完整读取或超出上限时明确报错，不返回局部倒序结果。酷狗使用原生最热、最新回复接口分页，不受这一完整线程排序上限限制。
- **酷狗音乐库为只读。** 支持读取“喜欢的歌曲”、个人歌单和歌单内歌曲，不提供收藏或歌单写操作。头像使用酷狗账号资料，可能与第三方登录账号的头像不同。
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
dist/org.yaqmc.providers.music-0.2.5.yaqmc-plugin
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

[MIT](LICENSE)。第三方协议参考及相关声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
