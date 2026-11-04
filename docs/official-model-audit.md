# 官方桌面模型与当前迁移证据

以官方 PC 3.1.42.205594 的恢复索引为分母，下表逐个列出 101 个命名空间。
索引包含 1681 个静态动作版本，其中有重复实现、内部 reducer、不同打包变体
和无动作页面。下表是模型级证据索引，逐动作验收仍须查专项报告，不能将版本数
当作独立功能数或通过数。来源是本机研究材料
`references/recovered-client/docs/product/model-index.txt`，不随产品发布。
该索引 SHA-256 为 `a3503d83e74d8625403737e9ceaa998962c045358630c0f0a0f5ad15e0dd18f9`。
本表基线为产品树 `e4964b50`；总体流程及验证层级见
[桌面验收矩阵](desktop-feature-matrix.md)。

「已接入口」只表示该模型有实际产品路径和可定位证据，**不**表示模型全部
动作完成；「部分」表示已有可用分支且仍缺独立分支；「仅原型」表示有隔离
实现但未生产启用；「仅恢复」只有恢复证据（可能已有其他模型的可复用能力）；
「宿主」和「待逐动作」表示应分别追踪平台适配和内部派发。每项的实服、
合成、Linux/Windows 和权限边界，以该行专项报告为准。

| 官方模型（动作版本） | 恢复卡 | 当前状态与证据 | 剩余或适用边界 |
| --- | --- | --- | --- |
| `$$MainWindow` (2) | `models/__MainWindow.txt` | [宿主](window-lifecycle-contracts.txt) | 主窗关闭、恢复；开机启动、冷/暖文件打开、Debian 打开方式候选分别验收，系统默认独立。 |
| `$$SubWindow` (15) | `models/__SubWindow.txt` | [宿主](desktop-lyrics-contracts.txt) | 浮动歌词及部分子窗；远程 H5 和桌宠不是同一链路。 |
| `@@dva` (1) | `models/__dva.txt` | [待逐动作](architecture.txt) | 官方框架内部动作；对照业务副作用，不要求移植 DVA。 |
| `@bff:modelForNej` (2) | `models/_bff_modelForNej.txt` | [待逐动作](architecture.txt) | 官方框架桥；尚无完整调用方和作用映射。 |
| `abtest` (18) | `models/abtest.txt` | [部分](official-pc-experiments.txt) | 完整 PC 请求、网页分组和原生签名读取有隔离证据；尚未替换各业务路径。 |
| `app` (54) | `models/app.txt` | [部分](window-lifecycle-contracts.txt) | 本机宿主和菜单；未覆盖每个 modal、原生回调或授权交接。 |
| `assignedAudioEffect` (10) | `models/assignedAudioEffect.txt` | [部分](effects-directory-desktop.txt) | 已接已验证材料的授权和处理反馈；逐歌、设备完整图待核。 |
| `async:action` (20) | `models/async_action.txt` | [待逐动作](catalog-navigation-ui-contracts.txt) | 部分资源有各自类型化入口；未完成统一派发器逐动作映射。 |
| `async:cloudList` (44) | `models/async_cloudList.txt` | [已接入口](desktop-cloud-contracts.txt) | 列表、上传、删除和播放；附件与 OSS 的线上边界独立。 |
| `async:decorationDesktop` (42) | `models/async_decorationDesktop.txt` | [仅恢复](desktop-decoration-contracts.txt) | 无正式动态桌面宿主、使用/停用和恢复。 |
| `async:h5Page` (2) | `models/async_h5Page.txt` | [仅恢复](settings-support-contracts.txt) | 有个别固定浏览器交接；无通用内嵌远程 H5。 |
| `async:hostResource` (55) | `models/async_hostResource.txt` | [已接入口](collection-library-contracts.txt) | 歌单、喜欢和收藏等有独立入口；未覆盖所有资源写回和购买。 |
| `async:listenTogether` (12) | `models/async_listenTogether.txt` | [已接入口](desktop-listen-together-contracts.txt) | 单账号建房/退出；双账号实测暂排除。 |
| `async:listenTogetherPlayList` (24) | `models/async_listenTogetherPlayList.txt` | [已接入口](core-listen-together-shared-queue.txt) | 共享队列及权威回读已恢复；不等于多端实际同步。 |
| `async:listenTogetherPlayStatus` (13) | `models/async_listenTogetherPlayStatus.txt` | [已接入口](core-listen-together-incoming-playback.txt) | 接收与归约已实现；专有 IM/RTC 的生产约束独立。 |
| `async:listenTogetherPlayer` (11) | `models/async_listenTogetherPlayer.txt` | [已接入口](core-listen-together-progress-request.txt) | 原生时钟与房间本机控制；双端语音和播放未验。 |
| `async:localList` (38) | `models/async_localList.txt` | [已接入口](desktop-local-library-contracts.txt) | 扫描、本地队列、匹配；标签编辑和细分事件待核。 |
| `async:lyric` (11) | `models/async_lyric.txt` | [已接入口](core-lyrics-contracts.txt) | 多源、解析、同步；具体格式、版权与上报另列。 |
| `async:miniProgram` (8) | `models/async_miniProgram.txt` | [仅恢复](settings-support-contracts.txt) | 通用远程小程序宿主/桥未做；独立迷你播放器不算。 |
| `async:petDesktop` (18) | `models/async_petDesktop.txt` | [仅恢复](desktop-pet-contracts.txt) | 窗口/素材和透明探针；无正式桌宠。 |
| `async:petGenerate` (48) | `models/async_petGenerate.txt` | [仅恢复](desktop-pet-contracts.txt) | 预设、照片生成、抽奖等无生产流程。 |
| `async:playingListHandoff` (53) | `models/async_playingListHandoff.txt` | [已接入口](relay-contracts.txt) | 接续、回执和恢复；缺真实另一端记录。 |
| `async:update` (16) | `models/async_update.txt` | [部分](manual-update-contracts.txt) | Cumulus 显式版本检查；不移植官方升级器，不自动下载或安装。 |
| `audioEffect` (61) | `models/audioEffect.txt` | [部分](core-effects-material-routing.txt) | 算法、材料与部分实时处理；整图未接。 |
| `configCenter` (3) | `models/configCenter.txt` | [部分](core-pc-request-context-contracts.txt) | 只读配置/实验按业务接入；缺完整预加载与更新覆盖。 |
| `dbPlayingList` (15) | `models/dbPlayingList.txt` | [已接入口](core-playback-contracts.txt) | 原生队列、存储和恢复；保留与官方 SQLite 实现差异。 |
| `download` (48) | `models/download.txt` | [已接入口](core-download-service-contracts.txt) | 三路任务和处理；各资源权益、恢复与文件平台独立验收。 |
| `fmPlaying` (15) | `models/fmPlaying.txt` | [已接入口](desktop-fm-contracts.txt) | FM、隔离队列、反馈及回退；不等于全部专区。 |
| `historyList` (21) | `models/historyList.txt` | [已接入口](play-history-contracts.txt) | 历史来源、可见范围和账号隔离；不扩大远端删除。 |
| `host` (96) | `models/host.txt` | [部分](account-feature-audit.txt) | 登录资料、多个账号资源；并非每个 SDK、远端网页或同步动作。 |
| `page:album` (8) | `models/page_album.txt` | [已接入口](album-detail-ui-contracts.txt) | 专辑歌曲、信息、导航；其他分支按契约。 |
| `page:albumlist` (6) | `models/page_albumlist.txt` | [已接入口](collection-library-contracts.txt) | 收藏/已购专辑；服务拒绝与空分开。 |
| `page:algPlaylist` (9) | `models/page_algPlaylist.txt` | [部分](search-preview-ui-contracts.txt) | 区分搜索专题预览、整页、资源种类及菜单，不等于全组合。 |
| `page:artistDetail` (21) | `models/page_artistDetail.txt` | [已接入口](artist-detail-ui-contracts.txt) | 艺人资料/歌曲/专辑/视频导航；普通视频单独。 |
| `page:artistsGallery` (8) | `models/page_artistsGallery.txt` | [已接入口](artist-gallery-contracts.txt) | 官方筛选和分页；逐平台视觉另验。 |
| `page:audio` (20) | `models/page_audio.txt` | [已接入口](desktop-podcast-contracts.txt) | 播客和有声书列表；付费权益及全部推荐布局待核。 |
| `page:audioDetail` (24) | `models/page_audioDetail.txt` | [已接入口](desktop-podcast-contracts.txt) | 节目详情/权限/播放；下载及续播有独立证据。 |
| `page:banners` (9) | `models/page_banners.txt` | [部分](discovery-home-contracts.txt) | 首页已证实的轮播；广告、跟踪和第三方回填未复刻。 |
| `page:bind` (2) | `models/page_bind.txt` | [已接入口](account-binding-settings.txt) | 设置快捷标识、独立绑定页、按注册方式分组和官方 App 扫码交接；不接上游动态第三方地址，不在客户端内执行解绑、改密。 |
| `page:cloudDisk` (1) | `models/page_cloudDisk.txt` | [已接入口](desktop-cloud-contracts.txt) | 实际云盘页面；手动重新匹配没有证据。 |
| `page:comments` (3) | `models/page_comments.txt` | [已接入口](core-comments-contracts.txt) | 分页、楼层、赞和回读；富文本及平台差异待核。 |
| `page:community` (0) | `models/page_community.txt` | [已接入口](community-read-contracts.txt) | 关注动态、列表和详情；不等于全部社区平台入口。 |
| `page:dailyRecommend` (5) | `models/page_dailyRecommend.txt` | [已接入口](desktop-rankings-contracts.txt) | 日推和播放；生日及系统推送为独立分支。 |
| `page:decorationSkin` (0) | `models/page_decorationSkin.txt` | [部分](desktop-decoration-contracts.txt) | 随包主题、在线目录和预览；远程安装和挂件未接。 |
| `page:download` (9) | `models/page_download.txt` | [已接入口](desktop-download-contracts.txt) | 下载页与任务管理；不同资源授权单独验收。 |
| `page:essential` (6) | `models/page_essential.txt` | [已接入口](ordinary-essential-contracts.txt) | 普通精选四个正文分区、歌单菜单及有声书单期的下一首和下载；匿名、横幅、实验变体和其他卡片动作另列。 |
| `page:essentialPlaylist` (4) | `models/page_essentialPlaylist.txt` | [已接入口](playlist-detail-ui-contracts.txt) | 精品歌单分类、续页和详情；与普通精选区分。 |
| `page:event` (20) | `models/page_event.txt` | [已接入口](community-read-contracts.txt) | 动态详情/评论；嵌入卡和深链还有边界。 |
| `page:eventPublish` (12) | `models/page_eventPublish.txt` | [部分](community-composer-contracts.txt) | 草稿与经确认的纯文本/已核对附件，合成写入；图片/转发待做。 |
| `page:eventPublishSearch` (11) | `models/page_eventPublishSearch.txt` | [已接入口](community-composer-contracts.txt) | 附件搜索；只有明确类型可提交。 |
| `page:followFanList` (11) | `models/page_followFanList.txt` | [已接入口](profile-relations-ui-contracts.txt) | 关注/粉丝，访客、本人与隐私状态。 |
| `page:followNewSong` (10) | `models/page_followNewSong.txt` | [已接入口](follow-new-song-contracts.txt) | 新歌读取/播放与取消通知；不猜测视频。 |
| `page:historyPlaylist` (9) | `models/page_historyPlaylist.txt` | [部分](play-history-contracts.txt) | 主线最近播放多来源有入口；不由此推断官方每类历史布局一致。 |
| `page:historyRecommend` (6) | `models/page_historyRecommend.txt` | [已接入口](desktop-member-contracts.txt) | 会员历史日推的播放和状态，和普通播放历史分开。 |
| `page:homePage` (59) | `models/page_homePage.txt` | [已接入口](discovery-home-contracts.txt) | 经典首页区块；新首页、访客、实验和专有广告不通算。 |
| `page:latestAlbums` (7) | `models/page_latestAlbums.txt` | [已接入口](latest-releases-contracts.txt) | 地区/月分组、续页、详情返回。 |
| `page:latestSongs` (6) | `models/page_latestSongs.txt` | [已接入口](latest-releases-contracts.txt) | 地区/周分组、播放和个性化设置。 |
| `page:localMusic` (23) | `models/page_localMusic.txt` | [已接入口](desktop-local-library-contracts.txt) | 本地页、过滤、分组、扫描、CUE/NCM。 |
| `page:login` (7) | `models/page_login.txt` | [已接入口](desktop-login-contracts.txt) | 登录主流程；真人各挑战独立。 |
| `page:loginCaptcha` (5) | `models/page_loginCaptcha.txt` | [已接入口](desktop-captcha-contracts.txt) | 已恢复验证码宿主；合成验证不代表真人通过。 |
| `page:loginMail` (9) | `models/page_loginMail.txt` | [已接入口](email-login-contracts.txt) | 邮件及相应挑战；线上支持边界单列。 |
| `page:loginNickname` (8) | `models/page_loginNickname.txt` | [已接入口](nickname-editor-contracts.txt) | 昵称规则、确认与失败；不是所有注册渠道。 |
| `page:loginPhone` (19) | `models/page_loginPhone.txt` | [已接入口](phone-entry-contracts.txt) | 手机号、密码、SMS 和已恢复的复用挑战。 |
| `page:loginQrcode` (10) | `models/page_loginQrcode.txt` | [已接入口](refresh-token-issuance-evidence.txt) | 真实扫码下发并保存；异常可原地重试，legacy 与 JSK 的扫码后 801 分开；不证明真实刷新轮换。 |
| `page:loginQuick` (2) | `models/page_loginQuick.txt` | [部分](server-quick-login.txt) | 有恢复、候选隔离、快捷历史许可及服务判失效后的本机清理；主动退出不保留凭据。 |
| `page:messagesheet` (66) | `models/page_messagesheet.txt` | [部分](message-drawer-contracts.txt) | 读/附件、已有会话文本回复与删除；已接账号隔离的原生未读计数与顶栏红点，受信 WebView 恢复可重启同一计时器；无实时推送、新会话发送。 |
| `page:modListenToSong` (9) | `models/page_modListenToSong.txt` | [仅原型](song-recognition-authority.txt) | 实际封闭回采、指纹和请求对照；无生产采集、提交或独立域权限。 |
| `page:musicDesktop` (0) | `models/page_musicDesktop.txt` | [仅恢复](desktop-decoration-contracts.txt) | 装扮桌面与歌词窗是不同宿主。 |
| `page:myAudio` (16) | `models/page_myAudio.txt` | [已接入口](desktop-podcast-contracts.txt) | 我的播客/收藏/节目管理边界。 |
| `page:myFavorite` (8) | `models/page_myFavorite.txt` | [已接入口](collection-library-contracts.txt) | 收藏、已购及对应列表；每个资源写入另验。 |
| `page:myHistoryAudioOnline` (3) | `models/page_myHistoryAudioOnline.txt` | [部分](play-history-contracts.txt) | 声音类历史有独立播放身份；未完成官方该页各态逐一对照。 |
| `page:myLikedVoice` (5) | `models/page_myLikedVoice.txt` | [已接入口](desktop-podcast-mutations-contracts.txt) | 喜欢声音与写入恢复。 |
| `page:myRecentUpdateVoice` (4) | `models/page_myRecentUpdateVoice.txt` | [已接入口](podcast-recent-contracts.txt) | 订阅更新及游标；红点和部分旧版逻辑待做。 |
| `page:newUserFavor` (9) | `models/page_newUserFavor.txt` | [已接入口](artist-onboarding-contracts.txt) | 按触发条件进入偏好引导；外部实验状态独立。 |
| `page:playlist` (29) | `models/page_playlist.txt` | [已接入口](playlist-detail-ui-contracts.txt) | 详情、分页、播放和管理；作者/分享等分支另验。 |
| `page:playlistsquare` (18) | `models/page_playlistsquare.txt` | [已接入口](playlist-detail-ui-contracts.txt) | 广场、普通分类与精品独立游标。 |
| `page:ranklist` (4) | `models/page_ranklist.txt` | [已接入口](rankings-ui-contracts.txt) | 榜单首页和各已核对入口。 |
| `page:searchResults` (20) | `models/page_searchResults.txt` | [已接入口](search-results-ui-contracts.txt) | 分类、综合、分页、热点播放及受版权约束的替代 MV；其他特殊资源和实验另列。 |
| `page:setting` (0) | `models/page_setting.txt` | [已接入口](settings-mapping.json) | 独立子项证据；页面存在不等于所有设置已实现。 |
| `page:settingAbout` (1) | `models/page_settingAbout.txt` | [部分](settings-support-contracts.txt) | 本机诊断与版本；无官方客服和自动日志上传。 |
| `page:settingAccount` (1) | `models/page_settingAccount.txt` | [已接入口](account-feature-audit.txt) | 账号资料、登录设备、绑定入口和海外注册手机号的扫码改密交接；各真实写操作分开。 |
| `page:settingNormal` (5) | `models/page_settingNormal.txt` | [部分](desktop-autostart-contracts.txt) | 启动窗口、本机自启动及 Debian 候选已接；未修改系统默认。 |
| `page:settingPrivacy` (3) | `models/page_settingPrivacy.txt` | [已接入口](account-settings-contracts.txt) | 六项远端设置、联动与回读；系统唤起交接另验。 |
| `page:singleIndexList` (2) | `models/page_singleIndexList.txt` | [已接入口](rankings-ui-contracts.txt) | 歌曲指数榜独立于普通歌单。 |
| `page:sns` (2) | `models/page_sns.txt` | [仅恢复](sns-login-recovery.txt) | 微信、QQ、微博授权宿主没有正式实现。 |
| `page:styleRecommend` (5) | `models/page_styleRecommend.txt` | [部分](style-recommendations.txt) | 风格页、种子、分类、明确保存/未决核对及 Back 恢复已接；快照失效按原路线读取。 |
| `page:switchAccount` (5) | `models/page_switchAccount.txt` | [已接入口](server-account-switch.txt) | 本机多账号与部分服务切换；真实双账号及 SDK 仍有限制。 |
| `page:trashCan` (3) | `models/page_trashCan.txt` | [已接入口](fm-feedback-ui-contracts.txt) | 音乐黑名单分页、选择、恢复及未知结果核对。 |
| `page:updateDownload` (1) | `models/page_updateDownload.txt` | [仅恢复](manual-update-contracts.txt) | Cumulus 显式读版本和校验；无官方更新下载页。 |
| `page:userDetail` (73) | `models/page_userDetail.txt` | [已接入口](account-profile-navigation-contracts.txt) | 本人/访客的资料和分区；未覆盖全部 73 个变体。 |
| `page:verticalZone` (3) | `models/page_verticalZone.txt` | [部分](desktop-fm-modes-contracts.txt) | 专区 FM 已接；完整专区歌单、专辑、榜单页未做。 |
| `page:videoDetail` (16) | `models/page_videoDetail.txt` | [部分](desktop-mv-contracts.txt) | MV 实际播放；普通视频未找到独立授权链。 |
| `page:vinylEffect` (10) | `models/page_vinylEffect.txt` | [部分](effects-directory-desktop.txt) | 音效目录、已支持的控制和授权；各材料与链路不同。 |
| `page:vinylPage` (24) | `models/page_vinylPage.txt` | [部分](desktop-member-contracts.txt) | 会员权益、部分动态开通；无原生收银台/兑换。 |
| `page:vipEssential` (3) | `models/page_vipEssential.txt` | [已接入口](desktop-member-contracts.txt) | 会员精选相关模块；并非所有投放与领取。 |
| `pages:loginDevice` (5) | `models/pages_loginDevice.txt` | [已接入口](account-profile-devices.txt) | 额度、原地展开/收起、详情和移除；未对真实用户设备执行删除。 |
| `playing` (78) | `models/playing.txt` | [已接入口](core-playback-contracts.txt) | 原生播放、队列和恢复；普通歌曲来源预判、底栏跳歌命令及队尾规则有对照；未由正式播放器自动触发。 |
| `playingList` (43) | `models/playingList.txt` | [已接入口](playback-queue-ui-contracts.txt) | 队列操作、本地单行和普通在线歌单多选拖入；普通队列的[更多推荐](queue-more-recommendations.txt)已接自动补歌、开关、分隔和自然续播。同步、特殊队列和其他来源另验。 |
| `setting` (45) | `models/setting.txt` | [部分](settings-mapping.json) | 设置逐项证据，系统相关和音效部分未完成。 |
| `sidebar` (14) | `models/sidebar.txt` | [已接入口](sidebar-ui-contracts.txt) | 可折叠列表、创建、导航；动线和实验继续逐项核。 |
| `window:moveDownload` (3) | `models/window_moveDownload.txt` | [已接入口](core-download-directory-contracts.txt) | 任务锁定、搬移及恢复；平台文件系统故障另验。 |

后续验收以行为边界为单位：

- 播放与搜索优先核对源入口的行为。例如播放列表接收其他页面的条目，
  与列表内部排序、默认添加到下一首、真正替换播放队列分别验证；热点卡的
  播放按钮、进入播放页和外链不能共用未经核对的动作。
- 系统开机启动由系统注册状态决定，和窗口显示方式、关闭时隐藏、程序
  启动时播放歌曲、默认播放器四条链区分。CI 也区分普通可执行文件、便携
  包与已安装包；原生启动成功不等于已经输出音频。
- 动态壁纸、头像挂件、桌宠、远程小程序、第三方登录和识曲各自有独立宿主
  或授权条件；不以已实现的主题、桌面歌词、网易云扫码或设备选择冒充。
