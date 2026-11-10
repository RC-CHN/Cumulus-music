# 桌面功能与验收矩阵

核对基线：`4c07fc7`（2026-11-04）。这是开发主线的核对索引；发行包能力按 [0.1.0 发行说明](releases/0.1.0.txt) 查询。支持 Linux x86_64、Windows x86_64。

[官方模型清单](official-model-audit.md)单列每个已恢复命名空间、现有路径及未覆盖分支。官方来源相对本机的 `references/recovered-client/docs/product/`（下表“报告”），界面以 `references/ui-recovery/sources/packages/pc/src/` 为主。原始材料不随应用发布；接口字段、请求顺序、失败分支和补充来源见每行的契约。已恢复实现及文件哈希另见 [来源索引](migration-source-index.json)。[分层说明](architecture.txt)、[逐页对照](ui-ux-audit.txt)、[窗口实测](ui-ux-validation.txt) 和 [Rust 验收](rust-core-validation.txt) 提供更细的证据。

“实服”指表中注明的具体授权样本；“桌面”指实际 Tauri WebView；“合成”可能包含真实 libmpv、SQLite、回环 HTTP 或虚拟音频设备，但没有访问真实账号资源。不同时间、平台或分支的证据不能合并成整项通过。

[交付证据索引](desktop-delivery-checkpoints.txt)按正式程序、合成实窗、实服和发行附件分别核对精确源码、可检查报告及尚缺的证据。

RC.2 后的独立候选已在真实 Linux WebView 验证 [普通播客下载音质](desktop-podcast-contracts.txt) 和 [历史批量添加到播放列表](play-history-contracts.txt)：取消不创建下载，明确选择后才授权并发布；历史批量追加保留已加载条目与暂停进度。证据是 `.build/ui-proof/phase257-podcast-history/` 的 37 张截图和对应原生报告。音效另有 [现代限幅器离线图对照](core-effects-modern-limiter-renderer.txt)；不代表生产播放器启用了该预设。

## 2026-11-09 集成验收

集成源码 `d6d6664d7ad27ed5eef82e596410099f50150baf`（本机；不等同于 RC 的主线提交）。锁定 Rust/Node 下，格式、严格 Clippy、Rust 2718 通过／27 忽略、前端 1008 通过、类型检查和构建均完成。实窗为隔离 Linux GTK/WebKit 和原生 libmpv；账号、媒体、写入及 Windows 端点均为合成，未接实体声卡或 Windows 设备。哈希、逐态 JSON 与原图在 `.build/ui-proof/phase134-combined/`。

| 本批交互 | 证据与剩余边界 |
| --- | --- |
| [下载行收藏](download-row-collection-contracts.txt) | 同一桌面构建已有目标 7 态、新建 7 态、系统「我喜欢的音乐」6 态。创建回执缺失时不追加；追加回执缺失只核对；系统列表和点赞列表一致才更新红心。单个窗口的目标互斥由原生测试验证。归因缺可信播放和动作引用，明确不补造。 |
| [播放栏低音量提示](windows-system-master-volume.txt) | 同一桌面构建 11 态，覆盖合成低音量/静音/30% 边界、跳转设置、冷却及原生 Linux 无能力。另有[音质菜单](playback-quality-dynamic-ui-contracts.txt) 9 个独立实窗状态，确认分组、并排/单行和高度；这些结果不等于 Windows 实机输出。 |
| [完整主流程](desktop-main-flow-proof.txt) | 同一桌面构建 HTTPS 硬切断 19 态、18 张 PNG，含登录恢复、日推、搜索、播放、歌单修改、下载及断网播放和清理；仅隔离合成写入，不代表该版本已在实服账号完成全流程。 |

本批之后独立复验：[云盘容量提示](cloud-song-table-ui-contracts.txt) 9 个隔离实窗状态、无购买或版本读取；[创建前读失败](download-row-collection-contracts.txt) 8 个隔离实窗状态、明确未发送后允许重新确认。两项报告与上表完整构建的证据分开。

后续集成源码 `f477708f05d6b12fb3a0122aeb7f503519404d7b` 又完成格式、严格 Clippy、Rust 2723 通过／27 忽略、前端 1009 通过、类型与构建。同一隔离 Linux WebView 构建验证云盘容量 9 态、云盘列表和选择 32 态、下载行创建 8 态与系统喜欢歌单 6 态，见 `.build/ui-proof/phase137-combined/`。

另行合入的专项：[新云盘音频上传的 100 MiB 限制](cloud-upload-form-ui-contracts.txt)在 Linux 实窗验证全部拒绝与混合过滤，原生另验证恰好上限及旧任务继续；[均衡器重置](desktop-equalizer-contracts.txt)在同一原生播放中解除总旁路并保留预设和队列；每日推荐启动通知仅在通过策略的主窗前台展示，独立实窗验证了播放、详情、设置、关闭、免打扰、禁用和三项抑制条件（合成账号）。不把专项运行当成 Windows 实机或真实账号证据。

隔离集成树 `f47743c2` 使用同一前端构建验证 [版权回归通知](account-settings-contracts.txt) 8 态、每日推荐通知 9 态和 [播客单集行菜单](podcast-program-row-menu-contracts.txt) 7 态，合计 34 张 Linux WebView 截图，另通过严格 Clippy、45 项定向 Rust 测试及 1011 项前端测试。证据在 `.build/ui-proof/phase145-notices-program-menu/`。后续独立复验先准备再打开的节目菜单 7 态，以及包含无歌单 ID 的版权回归 11 态和 3 个每日推荐回归场景；无歌单 ID 可播放和处理通知，未知可见性与幂等性的创建请求没有接入。两批均为合成账号和媒体；对应产品已合入主线 `a4b42feb`，RC 是否包含这批功能以实际发行标记和产物为准。

下一批集成树 `8deb7f1e` 的[最近更新单集菜单](podcast-program-row-menu-contracts.txt)绑定列表版本和节目 ID，复用 Rust 的下载、评论和队列用例；定向 Rust 与 Linux 实窗验证分页后丢弃旧回包、暂停时插入下一首、直接单集播放和公开链接分享。[前台定时通知](account-settings-contracts.txt)按已恢复的半点、抖动、共享计数、优先级和凭据隔离执行，由原生合成时钟验证，不把周期测试说成后台系统推送。两者均未用真实账号验收，详见[交付证据索引](desktop-delivery-checkpoints.txt)。

以下继续保留基线和各专项的证据，不由本批覆盖其他业务。

后续本机候选分别核对了 [搜索声音菜单](search-card-ui-contracts.txt) 和
[首页回忆引导](discovery-home-contracts.txt)。前者在真实 Linux WebView
确认「播放」不会暂停正在播放的同一声音，暂停时沿用原生实例续播，行号
仍能暂停；评论数和完成文件目录分别核对官方参数与原生交接。后者把
「引导消失、播放器仍空闲」的截图放在真实点击播放之前。
[Windows 本地文件打开候选](default-player-contracts.txt)接入持有句柄的
MP3、FLAC 和 PCM WAV，提供同次便携包的原生复验入口；GNU 交叉检查和
Linux FFprobe 对照仍不能替代 Windows 原生验证。这些候选不视为 RC 已包含。

后续隔离集成源码 `8146a18d8f8e42c0a154e403fbe2bff2a6bf1d06`：独立 Linux
应用用本地 FLAC 真实播放和暂停，进度由原生音频推进，输出接隔离的 PipeWire
null sink。相同源码的 Tauri 探针重走 [HTTPS 主流程](desktop-main-flow-proof.txt)
19 态；另有 [批量取消确认](desktop-download-contracts.txt) 4 态、
[音量浮层滚轮](playback-volume-ui-contracts.txt) 40＋3 态及
[当前播客断点续播](player-podcast-resume-contracts.txt) 7 态，合计保留 77 张截图
（含正式程序窗口、本地导入、播放及暂停）。前端 1030 项、类型/构建及工作区严格
Clippy 已通过。合成登录含持久化与重开恢复，未使用真实账号；本批未测实体声卡、
Windows 运行和 Windows 视觉，发行状态以主线标签对应的 CI 为准。

下一批的 [下载批量收藏](download-batch-ui-contracts.txt) 和
[登录态播客搜索分页](search-results-ui-contracts.txt) 各有独立 Linux 实窗证据。
前者核对下载原始顺序、普通本人歌单、提交前权限及回执缺失后只读核对；后者
核对 0/10 与 10/10 的真实签名请求、失败保留与显式重试、空列表与 null 的差异，
以及详情返回后的焦点。其独立证据不作为 Windows 或集成构建的运行结果。

## 2026-11-10 实服与同窗口验收

[签名播放证据](signed-window-read-proof.txt)确认：同一个 Linux Tauri 窗口恢复既有授权账号，读取本人歌单和喜欢列表，从官方 V3 综合搜索取得歌曲，核对非试听授权，再通过一次完整媒体读取驱动原生 libmpv 解码、暂停、继续和停止。原账号和数据库不变。使用 null 音频输出，未验证实体声卡、受易盾令牌保护的单曲分类、实服写操作、普通实服下载或 Windows 便携包。歌单编辑、下载和真正断网后的本地播放仍按[独立合成主流程](desktop-main-flow-proof.txt)记录。

同日独立产品候选的[普通自建歌单断网回读](playlist-offline-contracts.txt)在 Linux
WebKitGTK/libmpv 中验证了 7 个快照/CUE 状态和 9 个本机下载状态。
明确拒绝撤销快照；可播放性按同账号的本机索引或已发布下载及权益校验，
外账号记录和被改写的文件不能播放。此批用合成账号、合成 WAV/CUE 和
null 输出，无真实音源授权或写入；加密下载另有 Rust 证据，Windows 实窗
及实服离线权益变化未在本批验证。详见
`.build/ui-proof/phase237-offline-download-playlist/receipt.json`。

下一批[合并实窗](desktop-main-flow-proof.txt)验证了全歌单时长排序、
在线混排 CUE 与离线下载、搜索组选后直接播放、两套耳机调音布局以及
一起听 488 退出核对。同源码另启动普通程序，以私有 PipeWire 测得真实
合成 PCM 和暂停静音。截图、哈希及后续旧房间回执防护的 Rust 检查分别
归到实际测试的提交；不作为 Windows 或双账号验证。

同一候选 `4861d330` 的[搜索批量下载](search-results-ui-contracts.txt)在 Linux
WebKit 实际核对六条路径，共 60 个截图状态：直达、已有歌单、系统喜欢歌单、
新旧任务混排、创建歌单、明确选择已购音质。测试检查合成 FLAC 帧、反向下载顺序、
原来的暂停播放、旧任务原 ID 与 Range 偏移，以及未收到回执时只读核对。
前端测试 1058 项通过；精确前端和原生探针哈希、逐态 PNG 和结果见
`.build/ui-proof/phase252-search-download-paid-integration/receipt.json`。
已购后新建的晚到/丢失回执、首轮只剩一首时原右击歌曲的判定、以及无已知长度
重新授权的传输恢复另有封闭原生回归，不能当成 Windows、真实会员授权或写入证据。

| 功能与官方来源 | 当前代码到桌面入口 | 已有证据与剩余边界 |
| --- | --- | --- |
| **协议和设备上下文**：报告 08–11，M/48、402、363、1368，Watchman/设备测量 | [签名、HTTP](../core/src/protocol/) → [身份](../core/src/identity/)、[Watchman](../core/src/watchman/) → [原生采集](../desktop/src/watchman/)和[启动配置](../desktop/src/identity.rs) | 冻结签名向量、配置缓存/取消/重开、Linux 原生采集和实际窗口 getter；实服有 PC 域受保护读取及隔离歌单 EAPI 往返。Windows 候选四条独立探针已有真实虚拟机只读观测；未接入产品默认签名，也未证明实体设备、完整官方引擎或全部风控分支等价。[请求契约](core-pc-request-context-contracts.txt) · [兼容映射](default-windows-compatibility.txt) · [原生诊断](windows-hardware-collector-plan.txt) |
| **登录、恢复与续期**：报告 01、08、22，M/363、1368 | [候选/凭据/持久化](../core/src/account/) → [登录、续期 IPC](../desktop/src/auth.rs)、[续期宿主](../desktop/src/renewal.rs) → [登录弹窗](../frontend/src/components/LoginDialog.vue)、[账号窗口](../frontend/src/components/AccountDialog.vue) | 有真实已有会话恢复及读资源。授权的独立 Rust 扫码会话已观察到 refresh token 下发、保存和新进程回读；正常续期因实验未启用而不发请求，独立诊断未取得成对轮换。二维码、手机号密码/SMS、邮箱、已恢复挑战、快捷登录/切换和失效/退出另按分支验证；部分真人挑战、原生 SDK 支路和服务端双账号切换缺线上验收。微信、QQ 登录是可选接入：已恢复官方入口和票据交换，尚未验证授权成功回调的来源和会话隔离，正式客户端未接入。[登录矩阵](account-login-desktop-mapping.json) · [续期](desktop-session-renewal-contracts.txt) · [下发证据](refresh-token-issuance-evidence.txt) · [SNS](sns-login-recovery.txt) |
| **账户资料、安全设备和设置**：报告 17、30，M/15、100、3101、3122 | [账号用例](../core/src/client/) → [资料、设备、设置桥](../desktop/src/settings.rs)、[头像](../desktop/src/account_avatar.rs) → [账号设置](../frontend/src/components/SettingsPage.vue)、[账号表单](../frontend/src/components/account/) | 资料/头像编辑与回读、登录设备列表/移除/SMS、三档会员额度对照及绑定页有独立契约。绑定页按已核验的注册方式分组，写操作转交官方 App。没有因此执行真实移除、解绑、改密；尚有 SDK 和各平台 UI 状态差异。[账号审计](account-feature-audit.txt) · [绑定](account-binding-settings.txt) · [资料/设备](account-profile-devices.txt) · [头像](account-avatar.txt) |
| **搜索、目录和导航**：报告 02、17，M/2245、597 及恢复 SearchBox/SearchResults | [搜索和资源](../core/src/catalog/) → [搜索 IPC](../desktop/src/search.rs)、[目录 IPC](../desktop/src/catalog.rs) → [搜索组件](../frontend/src/components/search/)、[目录页](../frontend/src/components/CatalogPage.vue) | 真实 v3 分流搜索、结果投影和原生播放有只读样本；合成桌面覆盖联想/输入法/历史、分类分页、返回及播放。未发现综合游标续页的官方调用；热搜有声书/播客入口已接，更多发现模块和官方布局分支逐项追踪。[搜索](search-results-ui-contracts.txt) · [发现](search-discovery-ui-contracts.txt) · [只读实服](search-playback-read-proof.txt) |
| **日推、排行榜、私人 FM 和场景 FM**：报告 02、19，官方 FM/排行模型 | [目录](../core/src/catalog/)、[FM](../core/src/fm/) → [排行](../desktop/src/rankings.rs)、[FM IPC](../desktop/src/fm.rs) → 日推、[排行榜](../frontend/src/components/RankingsPage.vue)、[私人](../frontend/src/components/FmPage.vue)/[场景 FM](../frontend/src/components/SceneFmPage.vue) | 实服已有完整榜单队列、FM 播放/反馈和场景目录样本；合成验证独立队列、自然续曲、失败恢复。背景、生日态及部分官方发现布局仍有差异。[日推与排行](desktop-rankings-contracts.txt) · [FM 模式](desktop-fm-modes-contracts.txt) · [反馈](desktop-fm-feedback-contracts.txt) · [场景](desktop-scene-fm-contracts.txt) |
| **歌单、喜欢、收藏和历史**：报告 16、17、27、30，官方歌单/收藏/历史模型 | [歌单](../core/src/playlist/)、[收藏/账号资源](../core/src/account/)、[历史](../core/src/history/) → [歌单](../desktop/src/playlists.rs)、[封面](../desktop/src/playlist_cover.rs)、[收藏](../desktop/src/collections.rs)、[历史](../desktop/src/history.rs) → 歌单页、[编辑](../frontend/src/components/playlist/)、[收藏](../frontend/src/components/CollectionsPage.vue)、[历史](../frontend/src/components/HistoryPage.vue) | 实服隔离私密歌单往返覆盖创建、元数据/封面、增删/排序/清空及回读；收藏和历史各有契约。未自动公开发布。原生本地 ID 保留原位置；已接通普通自建歌单的混合拖动，隔离 Linux 实窗验证两次合成更新及丢失回包后的只读核对。真实账号混排写、混合增删、未绑定本地占位和部分收藏/分享布局仍有独立边界。[混排](playlist-mixed-reorder-webview-proof.txt)[私密歌单](core-pc-playlist-official-contracts.txt) · [封面](desktop-playlist-cover-contracts.txt) · [收藏](collection-library-contracts.txt) |
| **队列、音源与播放控制**：报告 02、18、19，M/7、176、308 等 | [播放器、队列、授权和时间线](../core/src/playback/) → [播放器桥](../desktop/src/player.rs) → [底栏](../frontend/src/components/PlayerBar.vue)、[队列](../frontend/src/components/QueuePanel.vue)、[迷你播放器](../frontend/src/mini/) | 实服 CDN 样本及本地 NCM 有实际 libmpv；合成主流程验证普通 HTTPS、明确重试、EOF、暂停定位和离线播放，另有队列入口与焦点回归。音源拒绝不等于上游永久版权结论；官方跳歌实验/专有 SDK 错误码不能直接套到 libmpv。[播放](core-playback-contracts.txt) · [主流程](desktop-main-flow-proof.txt) · [优先项](playback-search-priority.txt) · [启动](playback-startup-contracts.txt) · [队列来源](queue-source-navigation.txt) · [空队列发现](queue-empty-discovery.txt) · [队列歌手](queue-artist-navigation.txt) · [队列喜欢](queue-song-like.txt) · [队列菜单](queue-catalog-menu.txt) |
| **会员、音质、试用和离线权益**：报告 19、21，官方会员/音质模型 | [权益](../core/src/membership/)、[音质](../core/src/playback/quality/) → [会员 IPC](../desktop/src/member.rs)、播放器 → [会员中心](../frontend/src/components/MemberPage.vue)、[音质](../frontend/src/components/PlaybackQuality.vue)、[试用](../frontend/src/components/QualityTrialDialog.vue) | 实服读取权益和资源降级样本；合成验证按曲资格、显式试用、未知结果与到期、下载离线限制。黑胶乐签卡片、历史及主动签到/未决回读已用合成桌面验证；礼券和动态开通仍有缺口。不执行真实支付，不推断所有会员能拿到相同音质。[会员](desktop-member-contracts.txt) · [动态音质](playback-quality-dynamic-ui-contracts.txt) · [试用](playback-quality-trials-contracts.txt) |
| **歌词与歌词上报**：M/370、21、3081 和官方桌面歌词桥 | [解析/投影/缓存](../core/src/lyrics/) → [播放详情](../desktop/src/player.rs)、[浮动歌词](../desktop/src/desktop_lyrics/) → [详情](../frontend/src/components/LyricsPanel.vue)、[独立歌词窗](../frontend/src/desktopLyrics/) | 有实服普通歌词及 CUE/侧车、MP3/FLAC/M4A 内嵌的本地证据；合成桌面覆盖 YRC、双向时差、账号切换、失效与恢复；候选保留空态字号并按当前本地匹配选择歌词，不更换音频。云盘非空歌词、SYLT/NCM 内嵌、真实上报的通过证据与普通歌词分开。[核心歌词](core-lyrics-contracts.txt) · [双窗口](desktop-lyrics-contracts.txt) · [上报](lyric-report-contracts.txt) · [公开贡献页](playback-detail-lyric-follow.txt) |
| **本地库、CUE、NCM、线上匹配**：报告 12、18、22，官方 localmusic/扫描模型 | [库与监视](../core/src/library/)、[音频容器](../core/src/media/) → [本地库 IPC](../desktop/src/local.rs) → [本地音乐](../frontend/src/components/LocalPage.vue)、[匹配](../frontend/src/components/LocalMatchDialog.vue) | 实际扫描、游客 CUE/NCM 解码、原生时钟/重开和匹配通过各自样本；分轨歌词加载前后的时间轴另有测试。标签编辑、更多格式与目录变更仍需逐分支验收。[本地库](desktop-local-library-contracts.txt) · [NCM](core-ncm-contracts.txt) · [匹配](desktop-local-matching-contracts.txt) |
| **下载、离线和文件管理**：报告 20、21、25、26、29 | [任务、传输、标签和离线](../core/src/download/) → [下载](../desktop/src/downloads.rs)、[文件管理](../desktop/src/file_manager.rs) → [下载选择](../frontend/src/components/DownloadDialog.vue)、[管理](../frontend/src/components/DownloadPage.vue) | 实服普通 FLAC/NCM、云盘原文件及已测节目/MV 分支；实际文件跨盘、目录迁移、重开续传和离线播放。默认普通音频、NCM 显式选择；确认公开歌曲的下载文件丢失后可显式在线播放。付费节目/特殊媒体和跨平台文件系统故障不以歌曲样本代替。[桌面下载](desktop-download-contracts.txt) · [下载](core-download-service-contracts.txt) · [文件](core-completed-files-contracts.txt) · [目录](core-download-directory-contracts.txt) |
| **云盘、NOS/OSS 与附件**：报告 15、23、24、29，M/3127 | [云盘与上传状态机](../core/src/cloud/) → [云盘 IPC](../desktop/src/cloud.rs) → [云盘](../frontend/src/components/CloudPage.vue)、[上传](../frontend/src/components/CloudUploadDialog.vue) | NOS 音频/封面、容量、播放/删除、下载和隔离往返有实服样本；OSS 签名、CRC64、分片、同对象凭据替换及故障恢复经对照和合成验证。实服未获得 OSS channel=3；LRC/CUE 仍受返回值限制，保留显式省略/恢复状态。[云盘](desktop-cloud-contracts.txt) · [上传](core-cloud-upload-contracts.txt) · [OSS](core-oss-upload-contracts.txt) |
| **播客、有声书和 MV**：报告 28–30，M/597、232、334 等 | [播客](../core/src/podcast/)、[MV](../core/src/mv/) → [播客 IPC](../desktop/src/podcast.rs)、[MV IPC](../desktop/src/mv.rs) → [播客](../frontend/src/components/PodcastPage.vue)、[发现](../frontend/src/components/PodcastDiscoveryPage.vue)、[MV](../frontend/src/components/MvPlayer.vue) | 实服有播客收藏/单集点赞、免费节目播放下载、MV 清晰度/收藏和文件操作样本；合成验证发现续页、回退、原生节目播放。付费节目/听书购买、播客顶部与类别发现的全量分支、其他视频/云贝未完成。[播客](desktop-podcast-contracts.txt) · [播客发现](search-discovery-ui-contracts.txt) · [MV](desktop-mv-contracts.txt) |
| **评论和楼层**：报告 32，M/21、438、232、15 | [评论](../core/src/comments/) → [评论 IPC](../desktop/src/comments.rs) → [评论页](../frontend/src/components/CommentsPage.vue)、[写入](../frontend/src/components/CommentWritePanel.vue) | 多资源列表/热评/楼层、隔离点赞有实服样本；合成桌面验证纯文本发表、回复、删除、未知结果只读核对。未用真实账号发评论，富文本与更多用户资料入口仍有差异。[读取](core-comments-contracts.txt) · [写入](core-comment-write-contracts.txt) |
| **同账号接续与一起听**：报告 06、31，M/401 与 async_listenTogether | [接续](../core/src/relay/)、[一起听](../core/src/listen_together/) → [接续桥](../desktop/src/relay.rs)、[一起听桥](../desktop/src/together.rs) → [接续提示](../frontend/src/components/RelayPrompt.vue)、[房间页](../frontend/src/components/TogetherPage.vue) | 同账号有只读实服与合成收发、上报和恢复；缺真实他端接续记录的端到端证据。一起听已恢复单账号 REST、IM/RTC 令牌、房间控制、队列与试听协调；实服双账号明确暂排除，专有 IM SDK 不随包提供，不把令牌回包当 RTC 双端成功。[接续](relay-contracts.txt) · [房间](desktop-listen-together-contracts.txt) · [原生 IM](desktop-listen-together-im.txt) |
| **音效目录、授权材料与 DSP**：报告 33–38，M/28、3070 等 | [材料、授权、NCAE 和 DSP](../core/src/effects/)、[安全音频插件](../core/audio-filter/) → [原生音效](../desktop/src/effect_activation.rs) → [均衡器](../frontend/src/components/EqualizerPanel.vue)、[目录](../frontend/src/components/EffectDirectory.vue) | 官方参数组合、材料、GEQ/BT、PEQ/旋转/动态及 SE 固定 bank1 有对照；SE1→默认限幅器仍是受限子图。实服有隔离非试用授权与材料样本；Linux 有 PCM、原生卸载、未决登记恢复和真实 GTK 后动态 SE1 边界验证。[受控 NCM-FLAC 源](stereo-pcm-source-validation.txt)已在真实 GTK 后测通增量子图、同实例控制、定位、真实尾音和撤销，并有独立无GUI core流程；该源仍显式测试选择，不接正常播放器设置或正式滑块，不等于完整官方图或一般音源。[普通本地 FLAC](ordinary-native-local-equalizer-proof.txt)的独立路径已在 Linux 桌面验证 GEQ/BT、暂停定位、独立 PEQ 交接和错摘要回退；在线与其他音源保留原路径。[索引 FLAC](indexed-flac-receipt.txt)另记录 Linux 普通本地库 1024/4608 帧连续输出、旁路、欠载定位及受保护源的独立边界。[SE 验证及边界](stereo-migration-validation.txt) · [DSP](core-effects-dsp.txt) · [组合](core-effects-compose.txt) · [空间](core-effects-spatial.txt) · [桌面](effects-directory-desktop.txt) |
| **宿主、动画、窗口与系统媒体**：报告 04、09，恢复 Header/left/PlayBar | [窗口与生命周期](../desktop/src/lifecycle/)、[平台媒体](../desktop/src/system_media/)、[快捷键](../desktop/src/shortcuts/) → [标题栏](../frontend/src/components/WindowControls.vue)、[侧栏](../frontend/src/components/sidebar/)、[设置](../frontend/src/components/SettingsPage.vue) | Linux 有原生启动、GPU 回退、主题、滚动与减少动态效果的实测；Linux MPRIS、Windows SMTC 与快捷键分别追踪。没有同状态的官方运行截图，不能从合成或虚拟桌面推断像素一致、全 DPI、实体刷新率或声卡听感。[UI 审计](ui-ux-audit.txt) · [媒体控制](native-media-controls-contracts.txt) · [Windows 边界](windows-portability-validation.txt) |

## 可运行基线与验收门槛

- 使用仓库固定的 Rust/Node 版本和锁文件。在工作区执行 `npm --prefix frontend ci`、`npm --prefix frontend run build` 和 `cargo build --locked -p cumulus-desktop --bin cumulus --features custom-protocol`；运行 `.build/target/debug/cumulus`。运行库、打包和精确提交 CI 规则见 [README](../README.txt)、[工作约定](../AGENTS.md) 与 [发行流程](releasing.txt)。
- 本机普通 Linux 可执行文件证据：`0e136bd` 的 `.build/target-main-integration/debug/cumulus`；`.build/current-linux-product.DAbXMB/isolated-ordinary/report.json` 记录真实窗口、隔离无外网、合成音频播放进度、暂停、PipeWire 虚拟输出中的 PCM 与静音。它是已核对的开发构建记录，并非 `4c07fc7` 的发行附件；后续构建可能覆盖可执行文件。
- `4c07fc7` 有 Linux 合成桌面增量证据：`.build/search-podcast-home-webview/audit.json`、`.build/queue-explicit-current-verified/audit.json`、`.build/lyric-two-way-proof.WwRw3Y/after/report.json`。主流程的最新报告版本及独立账号权限见 [桌面主流程](desktop-main-flow-proof.txt)。本机证据目录不随公开仓库分发，契约中保留复现入口。
- `e856fbb8` 在同一份 Linux 桌面构建中验证了[副歌点队列复用](desktop-player-contracts.txt)、[搜索联想三种文案](search-discovery-ui-contracts.txt)和[搜索优先播放已下载 FLAC 的音效与回退](core-effects-local-equalizer.txt)。A→B→A 只读一批；联想的候选文案、顶栏和实际请求分别核对；可信解码库启用本地音效，摘要错误用原文件回退，暂停定位不改变曲目或队列。隔离 WebView 用合成数据和 libmpv null 输出，共 20 张截图；已核对实际运行库来自主线 CI `42582603562` 第 1 次。未验证 Windows 实窗或物理音频输出。
- 本轮普通歌单播放补齐[红心与固定雷达的原生来源](core-playback-contracts.txt)，合成 Rust 授权回执核对 43／37、未知或其他账号的默认场景和队列条目保留；[FM 副歌](desktop-player-contracts.txt)使用各自补曲状态，不与普通队列或其他 FM 来源混用。[单曲搜索命中](search-results-ui-contracts.txt)和[听书进度显示](play-history-contracts.txt)各有隔离 Linux 实窗记录：前者检查同一行的别名和歌词、游标及播放队列，后者检查隐藏进度后真实章节仍可播放。当前批未验证真实账号、物理声卡或 Windows 实窗。
- 后续开发主线的专项证据见 [播放、音效与搜索](playback-search-priority.txt)：暂停倒拖、耳机调音对比、音效故障时保留播放、声音和播客分类、播客行内收藏、跨分类的搜索纠正。音效恢复仅在原生确认拆除故障滤镜时继续，已验证耳机型号的独立 PEQ 不代表完整的官方效果图。各项证据仍按相应提交、平台和运行条件读取。
- `e0099f1` 的合并版在 Linux 实际 WebView 验证 [歌手发现](artist-gallery-contracts.txt) 的官方筛选和跨批分页、[本地单曲菜单](local-resource-ui-contracts.txt) 的播放与下一首、[播放栏播客单集下载](desktop-player-contracts.txt) 的资源身份和显式确认。12 张截图及回执在 `.build/ui-proof/local-gallery-voice-e0099f1/`。歌手页跨过 200 条后仍按服务位置续读；本地菜单保持当前 CUE 条目的暂停位置；单集确认框打开和取消均未提交下载。它们分别使用只读合成服务或本地合成文件验证，不能替代实服权益或 Windows 实机验证。
- 后续补齐[歌单广场](playlist-detail-ui-contracts.txt)的官方推荐首页、顶部动态标签和普通分类的 60px 自动续页，播客播放栏确认后实际下载，以及[评论作者主页和回程](comment-read-ui-contracts.txt)。Linux 隔离桌面验证有界渲染、原游标重试、个性化关闭、发布一份合成 MP3 并继续播放，以及作者跳转后保留原楼层及草稿。真实节目权益与嵌入评论的回程仍按各自证据验收。
- [会员卡](desktop-member-contracts.txt)支持固定 App 扫码，并读取官方动态开通配置。已知公开的二维码和浏览器交接由原生处理；原生收银台、支付和 SSO 仍不在已验收范围。独立参数均衡和耳机调音的底栏[处理反馈](effects-directory-desktop.txt)已通过 Linux libmpv 专项，读取当前条目原生样本计数区分「已配置」与「正在处理」。
- [精品歌单](playlist-detail-ui-contracts.txt)已接通分类入口、独立标签、原游标续页和详情返回。[单曲和歌词搜索](desktop-player-contracts.txt)可暂停或继续当前正在解码加载的歌，Linux 隔离主流程核对播放实例、音源授权和加载次数不变。
- [乐签详情](desktop-member-contracts.txt)已接通原曲喜欢、暂停/继续、奖励进度和专用会员二维码；没有新增领奖入口。[前瞻限幅器](core-effects-material-routing.txt)已有原生对照的离线 PCM 内核、有限输入输出缓冲和尾部排空；正式播放器和完整音效图未接入。
- 歌单详情切换曲目、评论、收藏者不增加返回步骤，离开再返回时恢复页签和滚动位置。搜索声音的结果行和综合卡按单集 ID 控制当前正在加载的同一实例；Linux 隔离桌面验证首载期间暂停/继续，未重复解码加载。[歌单详情](playlist-detail-ui-contracts.txt) · [声音搜索](search-card-ui-contracts.txt)
- [消息中心](message-drawer-contracts.txt)已接通后台未读计数轮询、私信/评论/@我/通知及会话历史读取；打开会话立即清除对应联系人在本机的未读角标。[一键全读](message-read-all-contracts.txt)使用独立确认、持久回执和只读核对。会话/私信删除、会话内纯文本回复及一键全读均不以真实账号写入作为测试。[新歌发布](follow-new-song-contracts.txt)可从关注歌手页读取发布流，独立播放官方最新列表；取消新歌通知走单独确认与持久化回执。限幅流的[离线重置](core-effects-material-routing.txt)已与官方原生对照，实际桌面播放跳转不使用这条音效链。
- [关注动态](community-read-contracts.txt)提供只读列表、详情及评论分页；[用户关注和粉丝](profile-relations-ui-contracts.txt)区分本人、访客以及隐私拒绝；[艺人偏好引导](artist-onboarding-contracts.txt)只从官方登录入口或符合时限的原账号记录触发。这些路径以各专项实际桌面和封闭合成服务为证；不表示已完成社交发布、真实账号写入、Windows 实机或所有官方实验分支。
- 关注页的[新歌提示](community-read-contracts.txt)使用独立接口和账号访问时间；[已确认类型的消息附件](message-drawer-contracts.txt)可排入当前播放列表、打开资料页或定位评论，返回保留抽屉位置。未确认归属的云盘歌曲、普通外链和社区私密动态不猜测授权。
- [普通视频恢复审计](ordinary-video-pc-recovery.txt)复现了官方 PC 从卡片走入 MV 默认路由的行为，未找到可独立授权的普通视频播放链。[限幅器联合实验](limiter-native-pipeline-proof.txt)已在 Linux 跑通真实解码、有限缓冲、恢复限幅器和同一 libmpv 实例，验证跳转换代、尾帧及自然 EOF；正式播放器尚未启用该路径。
- [搜索歌单](search-results-ui-contracts.txt)按官方标题下的第二行展示接口推荐理由；[播放最新](follow-new-song-contracts.txt)保留重复选中首曲时的原进度，并在首曲不可播时保留原队列。[动态会员扫码](desktop-member-contracts.txt)使用独立窗口和当前投放凭据，旧版本无法生成二维码；未验证真实开通、支付或礼券。
- [听歌排行](profile-play-record-contracts.txt)恢复了用户主页入口、周榜和总榜、访客实时权限及本人播放次数。访客被拒绝时不请求榜单；同账号近期播放历史仍独立。[云盘元数据](cloud-metadata-recovery.txt)按官方规则补首位歌手，保留原云盘 ID；未找到 PC 端手动重新匹配链路。
- 集成树 `4925610430d3e1808c432ce8164ba4a0b19c61d7` 用同一套生产前端和
  原生探针实际核对了云盘三条流程：[歌词来源与空态/失败](playback-detail-lyric-follow.txt)
  6 态、[当前云盘曲定位](cloud-song-table-ui-contracts.txt) 28 态、
  [上传任务与原音频选中](cloud-task-table-ui-contracts.txt) 27 态。
  Linux 实窗共 61 态和 61 张截图；证据索引为
  `.build/ui-proof/phase202-cloud-combined/report.json`。定位实际居中，离开悬浮后
  约 3.06 秒隐藏，不改原播放项；上传任务的原生文件校验实际执行，系统文件管理器
  使用测试替身。48 项云盘核心和 20 项下载文件测试通过，前端 1040 项及类型/生产
  构建、定向严格 Clippy 通过。不是 Windows、实体设备或实服账号证据。
- [普通云盘歌曲文件定位](cloud-menu-ui-contracts.txt)核对上传源、完成下载和
  扫描/CUE 三种原生凭据。Linux 实窗 32 态通过；生产 IPC 向隔离的
  FileManager1 发送恰好一次选中原音频的请求。115 项云盘及下载核心测试、
  Linux 严格 Clippy 和 Windows 交叉检查通过；未实际运行 Explorer。
- [搜索结果离线与恢复](search-results-ui-contracts.txt)的 Linux 实窗通过
  10 态：离线卸载结果，恢复当前分类、滚动和焦点，丢弃迟到回包，在线读取失败
  仍可显式重试。依赖 WebView 连通事件；官方业务探测计时和 Windows 未在此验证。
- [音效共用页头](effects-directory-desktop.txt)只在当前账号选中且确认配置
  本机均衡器时提供该开关。Linux 实窗 10 态验证跨页签开关、同一条目/暂停位置、
  一次解码和两次设置提交；独立 PEQ、权限和目录请求不走此开关。
- 集成树 `3aa38cd320ed102915cfafe3e8b02e5ca53e6df1` 的搜索断网、音效页头与
  [普通队列播客原文件定位](queue-voice-menu.txt)已在同一个
  编译版本复验，各 10 个 Linux WebView 状态。选中文件使用合成 FileManager1，
  不替换播放实例；无真实账号请求或写入。前端 1040 项、格式、类型及生产构建通过；
  30 张截图及联合报告索引为 `.build/ui-proof/phase203-broad-ux/report.json`。
- [播客和有声书分类](search-discovery-ui-contracts.txt)恢复官方评分；[播放最新](follow-new-song-contracts.txt)在首曲没有线上授权时仅使用当前账号同一歌曲已验证的下载。[进程内 FLAC](limiter-inprocess-decoder.txt)验证了专有前瞻限幅器前的解码和样本跳转，仍未切换正式播放器；网络授权与输入边界见[源契约](limiter-authorized-source-contract.txt)。
- [本人排行删除](profile-play-record-contracts.txt)支持按时期删除并在不确定时只读核对；[外部歌单](external-playlist-import-contracts.txt)支持按官方规则预览分享链接，未提交外部导入任务。[冷离线 FLAC](cold-offline-flac-handoff.txt)通过合成凭据、真实解码、原路径替换与会话撤销的专项验证，正式播放器未切换。
- [用户主页笔记](profile-notes-contracts.txt)独立读取作者列表，复用动态详情和评论，保留返回焦点；[社区编辑器](community-composer-contracts.txt)支持五类附件搜索及账号隔离的本机草稿。[排行卡](profile-play-record-contracts.txt)按作者播放周榜，同源暂停和恢复保留播放实例。三项已过 Linux 合并版 WebView，未执行真实发布。
- [冷离线限幅联合实验](cold-flac-native-sink-proof.txt)验证真实 NCM-FLAC 解码、限幅、输出及撤权；[正式原生停止](pinned-audio-teardown.txt)不能确认当前条目停止时会销毁原生输出。完整限幅路径仍未默认启用。
- [本人头像](account-profile-navigation-contracts.txt)与昵称分离，当前账号的笔记、关注、粉丝可直达对应资料页；[主页播客](profile-podcasts-contracts.txt)分开创建与收藏，保持同级页签历史及卡片返回。管理浏览器交接仅在显式点击时申请原服务票据。
- [冷离线跳转回退](cold-flac-native-fallback.txt)用原授权句柄验证了无索引和稀疏索引 FLAC 的原生回退：旧输出停止和解码线程交还之后才启动新输出，保留暂停/播放。仍未默认接管正式播放。
- [社区笔记发布](community-composer-contracts.txt)已接入正文和一个已核对的音乐附件，明确确认后才检查绑定并请求。成功用服务回执进入真实详情；回包未知只核对本机持久回执，不重发、不用正文猜测。验收只用隔离合成数据，图片与转发是另行分支。
- [新歌速递与新碟上架](latest-releases-contracts.txt)按官方地区、推荐开关及周/月分组读取，全部模式先取齐同月；隔离 Linux WebView 验证批量播放、首曲拒绝、原生双击、原游标重试和专辑返回。真实服务目录读取和 Windows 结果仍单独追踪。
- [听歌识曲](song-recognition-contracts.txt)已确认官方 PC 采集、时序、原生请求和 UI 状态；官方合成向量与 Rust 的默认峰值封包、AES 和 Base64 一致。完整声学前端、生产回采和真实识别仍未验收。
- [社区话题](community-topic-contracts.txt)从笔记内的 #话题# 进入，独立读取同一话题的热门笔记和全部笔记，恢复原游标及返回位置。前台新笔记提示不会自行刷新；参与和分享不在此入口。
- [本人笔记删除](community-note-delete-contracts.txt)只在同一账号拥有的卡片/详情显示，确认后发一次；明确回执移除对应列表条目，回包不明可显式读取同一原生详情。404 不确认删除，原生墓碑与提交回执分别记录。只用封闭合成账号验证。
- [动态和笔记举报](community-event-report-contracts.txt)绑定当前顶层动态及作者，确认后单次提交；状态不明只保留本机回执。Linux 封闭桌面核对小窗口菜单与旧删除流程，没有真实举报。
- [外部歌单导入](external-playlist-import-contracts.txt)支持链接和粘贴文字，可新建歌单或导入「我喜欢的音乐」；提交前单独在 Interface 域扫码核对同一账号并确认目的地。服务任务受理与导入完成分开，未知回包及重开不重发。图片选取、官方上传回退及任务回执已有合成对照，但独立 MP 授权未证实，图片入口和服务端完成查询未接入。
- 侧栏新建菜单按官方实验分组开启导入入口，缺失或不确定的分组保留直接新建；一次性提示独立落盘。实验回包和导入授权分别验证，未读取真实分组。
- [歌手 MV](artist-mv-discovery-contracts.txt)按官方 MV 数量门槛显示页签，用原游标续页和重试。Linux 合成桌面验证精确评论线程、原生视频画面和返回焦点；普通视频与全站 MV 发现没有用歌手列表冒充。
- [识曲峰值选择](song-recognition-signal-contracts.txt)及[可选 zlib 封包](song-recognition-contracts.txt)有官方原生合成对照。峰值阶段接受已中心化矩阵，不接设备采集、FFT 或真实服务。
- [经典发现首页](discovery-home-contracts.txt)接通 11 类可确认的官方区块。首页与目录详情往返保留横向和纵向位置及焦点，隐私关闭时不继续请求个性化推荐；游客和实验布局按独立证据追踪。
- [随心听 FM 封面取色](scene-fm-palette-contracts.txt)迁移 NOS 头、RGBA量化、HSV、原版渐变和主题底色；取样保留本地 WebView canvas，未冒称另一种缩放器等同原版。Linux 封闭原生媒体验证四类颜色流程，节目详情按官方隐藏来源行。
- [经典播客首页随心听 FM](podcast-home-scene-fm.txt)接入完整队列、播放设置与首次自动播放。新版不加载该卡；经典回退允许手动播；Rust 保留正在播放和较新的控制。Linux 真实 WebView 和合成音频通过自动、手动、更新布局及目录失败刷新，标题验证保留播放实例。
- [播客最近更新](podcast-recent-contracts.txt)用官方更新游标读取订阅单集；Linux 合成桌面验证原生声音、暂停、两种双击队列策略和返回位置。逐播客更新红点和旧版推荐仍有独立边界。
- [离线识曲声谱](song-recognition-signal-contracts.txt)从已经转换好的 8 kHz 单声道 PCM 到封包经过官方对照。[设备回采与请求](song-recognition-host-contracts.txt)目前有恢复契约，尚无产品识别。
- [榜单精选和首页风格卡](discovery-home-contracts.txt)使用官方实验和原生种子，真实 PC 会话只读样本显示六张榜单卡。
- [用户黑名单](user-blacklist-contracts.txt)可从隐私设置读名单及从资料页加入、解除，未知回包只读核对。实服只读空名单，增删使用合成账号。
- [本机网络诊断](settings-support-contracts.txt)不依赖账号，只检查环回、路由和代理变量；不联系主站、收集日志或发反馈。
- [识曲重采样对照](song-recognition-resampler-proof.txt)冻结官方 DLL 和公开同 ABI 的 24 个输入用例，当前尚未接入回采或真实识曲。
- [最近常听的风格卡](discovery-home-contracts.txt)进入官方普通风格页，不复用精选卡的种子。
- [个人主页举报](profile-report-contracts.txt)支持四种预设和自定义原文、明确确认与仅本机回执。只验证合成提交，不代表真实举报。
- [评论及楼层举报](comment-report-contracts.txt)接通官方 PC 路径；取消不发请求，丢失回执不自动重发。Linux WebView 封闭验证，不代表真实举报已发送。
- [手动检查更新](manual-update-contracts.txt)读取 Cumulus 已发布的稳定版元数据并允许打开固定发布页；用户可离线比对本机安装包的 SHA-256，不下载、不安装。Linux 合成桌面与 0.1.0 匿名元数据样本分开记录。
- [可选识曲重采样与私有回采](song-recognition-monitor-proof.txt)支持本机 Linux 合成输出对照，尚未接正式入口、真实设备或识曲服务。
- [识曲请求调度](song-recognition-request-proof.txt)按累计 3、6、9、12、15 秒前缀生成请求。封闭 PipeWire 探针已从真实 i16/f32 流走到指纹、EAPI 报文和进程内回包，验证取消后的迟到结果。正式入口、真实用户设备和上游识曲未启用。
- [识曲权限与采集选择](song-recognition-desktop-contracts.txt)保留候选字符串 ID 和顺序，按当前账号及识曲代次读权限，不返回授权地址。可选 Linux 宿主仅在私有合成输出验证选择许可，未确认不采集，所选输出移除或服务重启后不回退。正式桌面入口、真实设备及真实识曲仍待验证。
- [Windows WASAPI 回采原型](song-recognition-wasapi-proof.txt)的设备选择和停止策略有六项合成测试及 Windows GNU 交叉链接证据。未在 Windows 实际回采或发布入口。
- [官方桌宠](desktop-pet-contracts.txt)已核对入口、领养状态、窗口动作和素材约束。本机 Linux WebKit 未通过半透明 WebM 探针；尚无正式窗口、现有已领养宠物的合格素材或 Windows 原生验收。
- [换肤](desktop-decoration-contracts.txt)支持默认、经典红、酷炫黑及自定义按钮颜色。明确应用后跨进程恢复。登录后的在线目录只读，预览须明确点击，原始地址不进入前端；远程应用、购买、挂件和音乐桌面仍未接入。
- [最近播放记录](play-history-contracts.txt)的设置开关在本机按账号保存，首次关闭须确认。关闭只隐藏多端记录，重新开启后可合并；开关本身不请求或删除历史。
- [识曲原生身份构造](song-recognition-native-identity.txt)有官方合成对照。普通请求身份不会代用识曲的原生字段；未接真实授权、设备采集或生产请求。
- [识曲请求授权](song-recognition-authority.txt)区分官方 CEF 的匿名、登录和结果权限三条链；现有 Interface/PC 凭据不扩到独立识曲域，未开启真实发送。
- [响度标准化](loudness-contracts.txt)与已授权的 GEQ/PEQ 同时使用时，播放中开关在受信适配器上更新已有节点；换曲隔离旧增益。Linux 原生滤镜链有合成 PCM 对照，Windows 实机及物理声卡不以此代验。
- [福利活动和 WPS](member-welfare-cooperation-audit.txt)各有官方入口、请求与写入契约。福利只读入口仅向服务分组确认且有效列表非空的当前账号显示；切号拒绝旧回包。未启用动态跳转、领取或 WPS 合作流程。
- [Linux 图形回退](ui-rendering-validation.txt)的软件配置保留合成；X11 强制共享内存，Wayland 由宿主协商。X11 实际绘制导入弹窗，物理 Wayland 实际验证默认和 GPU/SHM 超时后成功回退。Windows 行为与设备限制不变。
- [精确基线 CI](https://github.com/RC-CHN/Cumulus-music/actions/runs/41763187918) 与已发布的 `v0.1.0` 是两套提交。只在 Linux、Windows、前端和最终 CI gate 均对**同一提交**通过后，才能用该提交的门槛作为发布证据。本轮没有发布新版本。
- [普通精选](ordinary-essential-contracts.txt)使用独立的官方歌单、最新音乐、精选有声书和热门播客，保留滚动和焦点；匿名注册、广告和实验排版分开。
- [自动启动窗口模式](window-startup-mode-contracts.txt)支持前台或留在托盘，托盘缺失会显示窗口；这个模式与系统注册、文件关联不同。
- [本地旁挂歌词](core-lyrics-contracts.txt)按原生路径顺序读取，Windows 的真实运行仍需验收。[淡入淡出](playback-fade-mute-contracts.txt)有官方 DLL 的离线数值对照；生产后端没有 DirectSound，未启用该设置。
- [系统开机启动](desktop-autostart-contracts.txt)读取当前用户实际注册状态，仅在明确操作时写本机并回读；默认文件关联独立。
- [本地拖入队列](queue-drop-contracts.txt)按落点插入或移动重复条目，保留当前解码和时间线；不接外部文件或链接。
- [搜索热点卡](search-results-ui-contracts.txt)接通歌曲和播客单集；封面控制音频、详情入口返回原搜索位置，未经确认的链接仍不打开。
- [普通歌单拖放](queue-drop-contracts.txt)保留筛选后的原序号和官方多选顺序。歌单版本过期时可显式重读，保留筛选和当前播放；由用户重新选中并拖动，不重放旧操作。
- [精选歌单卡片菜单](ordinary-essential-contracts.txt)接通查看、播放、下一首、收藏、分享、复制和下载全部；精选有声书的单期菜单只操作该期。下载仍须确认，不能把该期的相同编号误当歌曲或整个节目。
- [本地文件打开](default-player-contracts.txt)按原生命令行顺序打开经验证的 MP3/FLAC。Linux 冷启动和向同资料目录既有进程的暖交接分别通过真实窗口、私有 PipeWire 验证；无效暖打开不动现有队列。Debian 包登记 MP3/FLAC 的「打开方式」候选，已有显式默认不变；该基线 Windows 只有冷启动，尚缺原生实测；后续冷暖候选另按上文独立验收。
- [综合搜索替代 MV](search-preview-ui-contracts.txt)由官方 PC 配置、当前资源的版权状态和明确推荐共同控制；独立视频授权，返回保留搜索位置、焦点和音频队列。官方自动跳歌的整队列预检仍分开处理。
- [普通扫码](desktop-login-contracts.txt)的服务异常可原地重试，失败候选会释放；按原版区分 legacy/JSK 扫码后收到 801 的显示，不替换二维码或重复发并行轮询。
- [快速登录许可](quick-login-offer-contracts.txt)独立于启动自动登录和本机恢复；官方 PC 分组开启后才能显式加入，普通新登录不会自动发送快捷历史。主动退出仍清除对应凭据。服务判失效只删除对应快捷资格，保留本机账号和手动恢复。
- [PC 联网与消息计数](pc-playback-connectivity-contracts.txt)已有官方脚本轨迹对照和按账号生命周期的 Rust 执行器；正式桌面已接入账号未读计数与当前主 WebView 的受信恢复事件；原生网络桥及自动跳歌仍未启用。
- [Windows 原生文件暖转交实验](windows-native-open-lab.txt)覆盖登录会话、目录和管道帧，独立于正式程序与安装器；Windows 真机运行和音频暖转交仍未通过。
- [PC 实验分组](official-pc-experiments.txt)恢复了 165 个前端键与 25 个 x64 原生键的精确顺序、分组和回退。已通过隔离签名 EAPI 的真实线格式、30 分钟和账号取消对照；[正式调度及搜索显示](pc-presentation-lifecycle-contracts.txt)后来已接入，当前账号的真实分组仍需单独实测。
- [普通队列来源预判](playback-search-priority.txt)在 Rust 使用已落盘、账户有权的下载优先，核对在线权益及明确提供的同录音候选；[主窗底栏的手动方向操作](desktop-minibar-skip.txt)已接签名分组、当前文档联网和版权预判；自然 EOF、原生故障号和不确定条件另列，403/503 不当作版权限制。
- [续期状态回读](desktop-session-renewal-contracts.txt)在重启、重开弹窗或正常时钟变化后，按持久的登录/已确认续期时间计算 24 小时门槛。查看状态不发续期请求，真实令牌轮换仍待证实。
- [续期未确认](desktop-session-renewal-contracts.txt)保留当前登录，不把缺失回执标成会话过期；只有收到完整配对回执才提供只读身份核对。不重复发送续期。
- [风格推荐返回](style-recommendations.txt)恢复原来显示的列表及标签，不自动刷新；旧种子页获得新播放版本。显式刷新仍向同一来源请求。
- [登录设备列表](account-profile-devices.txt)保留前六台并原地展开，已退出的设备完整显示；详情退出后回到原行。未删除真实设备。
- 此矩阵仍需在交付审计前细化到其余已恢复模型/动作和未核对的界面状态。尤其要逐个验证尚未复刻的发现页/会员分支、专有效果图、外部账号或 SDK 条件和 Windows 原生界面；不能用已覆盖的入口或测试数代替这些要求。


以 2026-11-05 的开发提交 `9bbf21e6` 补充：

- [歌手卡片](artist-card-ui-contracts.txt)按官方实验分组读 Top50 或普通歌曲列表；Linux 真实 WebView 核对两种请求、队列顺序和失败时保留当前播放。
- [搜索单曲拖入](queue-drop-contracts.txt)通过 X11 真实指针输入验证多选顺序、落点、取消和原生暂停位置不变。
- [播客断点](desktop-podcast-contracts.txt)与[有声书跳过片头尾](audiobook-skip-ui-contracts.txt)接到原生播放时钟；Linux 合成请求和实际解码均有记录，未用实服验证本次上报或算法样本。
- [内嵌评论发表／回复](vinyl-comment-compose-contracts.txt)用真实 Linux WebView 验证当前歌曲约束、单次发送和丢回包后的独立页核对。没有使用真实账号公开发布。
- [已登记的普通参数均衡旁路](effects-directory-desktop.txt)按已获授权的实际音质触发，下一次成功装载恢复。Linux 合成 PCM 和桌面控件均已复验，未扩大至整张专有效果图。
- [下载批量页](download-batch-ui-contracts.txt)已覆盖实体删除和多选播放；批量移动不由单文件移动或整目录迁移代替。
- [内嵌评论举报](vinyl-comment-report-contracts.txt)在精彩和最新评论复用当前播放及同账号回执；Linux WebView 核对取消、预设、原文和回包不明时不重发。
- [会员权益自动读取](core-membership-contracts.txt)由原生登录生命周期触发；Linux WebView 核对首次加载和按官方日期规则复查，综合搜索即时更新，日期不变时不轮询。
- [在线播放缓存](streaming-media-cache-contracts.txt)为少数已获授权的普通 MP3 保留本进程临时字节；Linux 核对原生播放、再次授权、清理和设置持久化，未将缓存当作离线授权。官方动态策略、原生缓存格式和更改缓存目录不由本实现冒充。
- [旧版 PC 定期续期](legacy-periodic-renewal.txt)按已验证的官方分组和登录来源安排，和 JSK 续期互斥；Linux 合成冷恢复核对两组请求和持久结果，真实 RT 下发仍需服务回包证据。
- [黑胶相似推荐](vinyl-similar-songs-contracts.txt)按已核验分组显示在独立页签或旧版百科内。三种播放动作、独立读取、歌单展开和跳转都有桌面探针；当前首曲不重开解码由原生播放器用例验证。[音乐百科](vinyl-wiki-contracts.txt)用签名读取并显示正文，Linux WebView 通过新版和旧版共 23 个场景；二维码目标缺少可验证的服务样本。

以 2026-11-05 的 `cb24b2cc` 桌面示例与 `7299fe9e` 产品源码补充本批专项：

- [声音标题信息](voice-playback-header-contracts.txt)从当前原生节目投影主播与发布时间；主播进入用户资料页，缺少有效用户 ID 时保留纯文字，缺日期不补成今天。Linux 隔离 WebView 已完成卡片、底栏、声音详情、主播主页及回到详情的 5 阶段/5 张截图；下载来源证明、歌曲/CUE/冲突元数据的限制按契约分别验收，不用这组在线界面代替它们。
- [封面失败占位](player-cover-fallback-contracts.txt)覆盖场景卡、播放栏和播放详情；同账号同 URL 复用已有加载结果，失败后使用本地 Icon，不新增重试或影响播放。上述 5 阶段实际核对占位与持续播放，5 张原图均由主代理逐张查看；本地 Icon 与官方专有默认图片仍是明确差异。
- [场景 FM 取色](scene-fm-palette-contracts.txt)已完成颜色可在同账号同封面切节目时复用，包含默认底色和普通取色失败；原生过期/账号身份失败不作为完成结果。复用和过期分支由定向 Node 状态用例验证；该批 GUI 集成只重跑 PNG 本地 canvas→Rust 的 sample 分支，3 阶段/3 张截图、76 字节 PNG，旧批四场景记录独立保留。当时等待中的同封面切节目仍因旧节目凭据绑定而重新发起；这条在途差异的后续验收见下方 phase57 补充。
- [独立 SE bank-0/ambience=0](core-effects-stereo-zero.txt)提供安全 FFI 与离线 PCM CLI，另见[来源到实现映射](effects-stereo-zero-mapping.json)。冻结官方对照为 42 组流/378 块；本批实际 CLI 执行 9 块并按语义 JSON 核对一致。未接播放器或 UI，不包含 ambience=1、混响、任意参数银行或完整效果图；这些结果也不代表 Windows 运行或真实声卡听感。

以 2026-11-05 的 `e93204b8` 产品源码及 `47f2df99` 验证脚本补充 phase57 进度：

- [场景 FM 在途取色](scene-fm-palette-contracts.txt)已补齐同账号同规范封面切节目时的 header、image、canvas 在途复用。Linux 真实 WebView 三个新场景共12阶段/12张PNG，601→602不增加读取或取消；旧节目新请求仍严格拒绝。头命中1请求/0图片body，图片回退2请求/76字节PNG，实际512字节本地RGBA送Rust；取色read各1次、sample为0/1/1、cancel均0。原生播放ID和队列保持、位置前进，不将此说成解码器分配次数或物理声卡证明。
- 原四种取色场景已有完成记录；旧聚合在第五场景的Python封面URL期待错误处中止。仅按当前verify重新解析旧记录，得到 `.build/ui-proof/phase57-search-and-palette/recovered-original-four-report.json` 的4场景/11阶段/11PNG通过，启动新进程数为0。三种在途场景独立执行于 `palette-pending-r2/report.json`，两根共23阶段/23PNG；保留原失败且不算作七场景同一次完整运行。GUI未验证ABA、实服CDN、Windows实际渲染或真实声卡。
- [综合搜索单曲页脚](search-preview-ui-contracts.txt)接入独立 `PH-PC-search-result=t1` 门控，实际签名完整PC实验批次同时提供原三桶布局；控制组、未知或过时回包不产生新的歌词命中、别名和推荐原因入口。刷新失败按同身份最近一次全局成功分配回退，不复活搜索本地旧t1；该显示回退不把过期快照变成新鲜授权。真实签名链的10项原生传输用例与完整Rust2337项、严格Clippy通过；四种分组实际WebView共14阶段/14PNG通过，并核对真实底栏与原生播放一致。Windows运行仍须后续精确CI，不用GNU交叉检查代替。
- 此条曾记录 phase57 的缺口。后续 [原生实验生命周期](pc-presentation-lifecycle-contracts.txt) 已覆盖启动、切号、持久显示缓存和 30 分钟定时，四项搜索表现也已[联动当前及缓存页](search-reactive-presentation-contracts.txt)；其他实验消费者独立验收。
- 本批Node测试885项通过。上述取色GUI使用Linux真实GTK/WebKitGTK、隔离合成传输及null libmpv；LC_NUMERIC单独为C，界面消息和字符locale仍为en_US.UTF-8。完整工作区和搜索桌面验证的来源、失败修正与平台边界另见本批验证记录。

后续主窗验证补充：

- [听过的书](play-history-contracts.txt)由精选卡进入独立在线列表，20 条本地分页。隔离 Linux 桌面核对了节目和媒体 ID、两章队列及返回精选的分类、滚动和焦点。它与六类最近播放独立；官方本机按书 recent/around250 分支另列。
- [顺序队列自然播完](ordinary-native-eof.txt)在原生成功 EOF 后按经核验的版权分组继续、保留或停止；真实 Linux WebView 核对三组结果及对照组播完后再按下一首。缺授权证据、特殊音源和非顺序模式分开。
- [最近播放同步设置](play-history-contracts.txt)只对尚未明确选择的账号应用官方实验默认。真实 Linux WebView 核对打开设置页时的实时更新、首次关闭弹窗及手动选择优先，SQLite 只记录一次明确保存。
- [列表循环、随机和跳歌提示](ordinary-native-eof.txt)已在 Linux 桌面核对实际回绕顺序、对照组停止推进、原文提示的出现与到期。
- [听过的书续听](play-history-contracts.txt)在账号本机记录与服务行匹配同一章时读取官方相邻章节接口，从记录的整秒处续听；缺章不替换原队列。同书不同章继续用原章节列表。
- [本地 FLAC 暂停定位](ordinary-native-local-equalizer-proof.txt)修复原生新流启动等待；本机用 CI 包内同一解码器和实际 WebView 验证可信解码、摘要回退及暂停 PEQ 交接。CI 的旧版 libmpv 另验。
- [环绕深度与强度](ordinary-native-local-equalizer-proof.txt)接入符合条件的本地 FLAC 原生处理；Linux 实拍覆盖保存、暂停热调整、主开关、定位与自然结束。没有合格处理链的音源可保留设置，但不会显示为正在处理；另验 Windows 及其他格式。
- [默认下载音质](desktop-download-contracts.txt)按已核实的本账号权益列档，允许显式恢复失败的会员读取。Linux 实际桌面已验证一次单曲授权、服务降档、真实发布及默认值不变，另验 Windows。
- [综合搜索音质角标](search-results-ui-contracts.txt)按官方曲目规格投影，会员变化仅重读普通前六首。Linux 实拍核对更新、失败保留、滚动焦点和原生暂停时钟；[歌词](search-lyrics-ui-contracts.txt)按满批继续，短批保持本页状态并结束。
- [原生打开 PCM WAV](ordinary-native-local-equalizer-proof.txt)支持符合条件的 44.1 kHz 双声道 16 位文件。Linux 核对实际空间音效、暂停定位和自然结束，以及向现有进程交接后的测试输出；不改变系统文件关联，Windows 独立验收。
- [搜索单曲使用已完成下载](search-results-ui-contracts.txt)在 Linux 实拍中以普通歌曲身份直接解码文件，暂停仍不请求在线音源；损坏后重新选择才走一次授权。已核文件显示不可点击的「本地」，在线后恢复音质菜单。
- [均衡器拖动](desktop-equalizer-contracts.txt)仅预览，放开或键盘确认才保存。普通 Linux 程序经系统鼠标核对了低音和 500 Hz 的预览、各一次持久写入及继续播放。
- [普通本地音效](ordinary-native-local-equalizer-proof.txt)在 Linux 真实窗口核对 48 kHz WAV 和 FLAC 的实际原生采样率、暂停定位和尾部；两格式逐样本相同另有独立 Rust 证明。[受控 Stop 交接](pinned-audio-teardown.txt)核对旧源释放与无回执时销毁；正式在线音效仍保留原路径。
- [黑胶来源](desktop-player-contracts.txt)区分新搜索播放与从搜索暂停已播歌单曲目；[歌词署名](core-lyrics-contracts.txt)核对站内用户、缓存失败和成功省略的差别。两项均有 Linux 真窗口，Windows 另验。
- [专辑多选拖入](queue-drop-contracts.txt)在 Linux 实际窗口按选择次序、可见插入线和当前暂停时钟核对；队列提交后的普通下一首预取单独计数。
- [随机或列表循环播完后的底栏](desktop-minibar-skip.txt)使用原生结束回执，即使原播放标识已经清除。Linux 桌面验证了切换到顺序播放后的新队列版本及再次点击。
- [按播客或有声书保存排序](desktop-podcast-contracts.txt)在真实 Linux 桌面核对了详情重开、账号隔离、暂停队列不变，以及有声书全量和相邻请求的排序规则。
- [普通底栏方向键](desktop-minibar-skip.txt)已接官方 M308 判权：同一已知无版权候选在 t1 跳到后续可播行，在控制组保留当前播放。自然 EOF、完整 Windows 联网观察和真实账号分组仍独立验收。
- [搜索表现](search-reactive-presentation-contracts.txt)在原生凭据轮换时不把旧响应签发为新表现候选；综合端点在 Tab 返回后重新看全局分组。实际 Linux WebView 复验控制组与 t1、返回缓存、滚动与焦点，凭据轮换另有封闭签名对照。
- [专辑时长排序](album-detail-ui-contracts.txt)已在 Linux 实际窗口核对原序、升降序、同长顺序、返回恢复、筛选播放、同曲双击和整张入队；普通真实服务与 Windows 桌面结果另行核对。
- [专辑热度排序](album-detail-ui-contracts.txt)读取官方 `pop`，Linux 实窗已核对进度条、降序／升序／原序、同分稳定、整专播放与暂停时整专入队；沿用原生专辑快照和整专读回，未做真实账号或 Windows 专项验证。
- [本地文件夹定位](desktop-local-library-contracts.txt)在 Linux 真实窗口和私有文件管理器替身验证定位、有限父目录回退、失败提示和焦点回归。Windows 使用 Explorer 单项选中，尚缺实机证据。
- [索引 FLAC 音效](indexed-flac-receipt.txt)在 Linux 用可信解码器和故意错误摘要分别验证固定图与同句柄普通解码，以及暂停定位、继续播放和自然结束。系统运行时、CI 打包解码器与真实声卡是独立证据；Windows 索引分支未开启。
- [声音搜索作者](search-results-ui-contracts.txt)只有有效的官方嵌套作者身份生成公开主页入口。Linux 真实 WebView 验证单一作者路由、退回焦点和滚动位置，未请求媒体或改动队列。
- [队列收藏](playback-queue-collection-contracts.txt)的行内、右键和更多入口共用 Rust 选择检查。Linux 实窗核对真实鼠标单首、Return 多选、取消零写入和显式选择后的合成写入及回读；下载单曲及已关联本地音乐复用行内操作，未关联文件提示处理，不启动上传。[队列菜单](queue-catalog-menu.txt)另核对了目标列表、焦点与暂停位置。
- [播放来源名](queue-source-navigation.txt)由入队时已有的专辑、歌手、歌单详情提供。Linux 实窗已核对专辑与歌单名称，歌手由 Rust 定向测试覆盖。没有已验证名称时不从歌曲资料猜测，也不补请求。
- [播客推荐卡菜单](podcast-recent-contracts.txt)支持详情、播放、插入下一首、确认收藏、分享和复制。Linux 实窗核对了取消和原生队列插入；收藏写入由封闭 Rust 夹具验证，不使用真实账号。
- 播放详情的[所属播单卡](vinyl-podcast-collection-contracts.txt)使用当前节目的播单绑定，核对收藏、取消确认、旧单集和自有播单；[节目推荐卡](vinyl-voice-recommendation-contracts.txt)按六张本地换页，使用响应 ID 打开播单。[包含歌曲](voice-playback-songs-contracts.txt)在 Linux WebView 核对封面播放、单击选中、双击顺序、下一首、版权禁用、喜欢回读和切集拒绝旧动作；[节目封面](voice-playback-header-contracts.txt)另验方形布局、播放量、占位与主播页返回。上述使用合成服务与 libmpv null 输出，不视为实服权益、实体声卡或 Windows 实测。曲目下载和多选动作另列边界。
- [节目详情联合读取与动作回执](voice-playback-header-contracts.txt)按官方并行读节目、推荐和包含歌曲，成功后才共同显示。Linux 实窗验证了挂起、失败、重开和封面锚定；收到原生签名回执时可进入新主播或播单并收藏，旧回执失效。身份不匹配又没有动作回执时资料仍只读。
- [节目包含歌曲多选收藏](voice-song-collect-contracts.txt)按原始选中顺序和收藏权限加入已有自建歌单、已核验的系统喜欢歌单或显式新建的歌单。Linux 实窗验证取消不写入、回包丢失后只核对、原行焦点和队列不变。喜欢歌单须同时读回详情和喜欢列表；新建分开核对创建和添加，已确认的创建可以显式继续，明确的添加失败独立显示。未验证真实账号写入或 Windows 实窗。
- [节目内已购单曲下载](core-voice-song-purchased-download-contracts.txt)先明确选择音质，必要时再加入歌单，使用保存的目录与格式在节目页就地排队。Linux 实窗验证直接下载、已有歌单前置、音质取消、无目录取消，并核对两份合成 MP3 的实际文件；不代表真实付费服务或 Windows 实窗通过。
- [节目内歌曲多选下载](core-voice-song-group-download-contracts.txt)在原始触发行已确认喜欢时直达。普通组按保存音质排队；混合或全已购组共享音质弹窗，明确确认后全组用同一请求档位。Linux 实窗验证取消不入队、混合组三首合成 MP3 落盘、无目录需显式打开设置，原节目及队列不变。未喜欢触发行所需的普通歌单、系统喜欢及明确新建已接通；合成 Linux WebView 另验证普通明确否续下、双来源只读核对及新建后加歌失败不入队。云盘和未购权益仍未接入，未验证真实服务或 Windows 实窗。
- [节目收录歌曲多选](voice-playback-songs-contracts.txt)按选择顺序将多首歌曲入队，右键固定所选组，双击遵循替换或追加设置。Linux 实窗用 X11 指针验证了菜单、暂停队列、两种双击偏好和单曲封面，未将组收藏和组下载计入通过。
- [节目收录歌曲单曲下载](voice-song-download-contracts.txt)区分已喜欢、已有目的地和新建歌单，只有原生授权后才建立普通单曲任务。Linux 实窗验证五条路径，包括取消、写入失败和只读核对；两次实际传输的文件与合成音频逐字节相同，并从已完成列表实际离线播放其中一首。
