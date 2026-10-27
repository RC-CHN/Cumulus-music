Cumulus Music

发行目标：Linux x86_64 与 Windows x86_64；不支持 macOS。
安装包：https://github.com/RC-CHN/Cumulus-music/releases
版本边界与安装说明：docs/releases/0.1.0.txt。
贡献/版本规则：AGENTS.md；CI/CD 与发布步骤：docs/releasing.txt。
CI 将前端检查、Linux 原生验证/打包、Windows 原生验证/打包分配到不同 runner。
仅在目标 release commit 的 CI 全部通过后发布同一次 CI 的构建产物。

跨平台社区音乐客户端，采用 Tauri 2 + Rust + Vue 3 / TypeScript。
新旧301校验、官方双实验开关和失效提示已接通；旧版明确清队列与恢复见
docs/login-legacy-contracts.txt，当前凭据发放边界见
docs/login-interception-contracts.txt与docs/refresh-token-issuance-evidence.txt。
账号JSK续期已接入原生前台触发、24小时间隔及账号窗口，含未决写入核对/跨重启恢复；
真实会话缺刷新凭据时跳过，详见docs/desktop-session-renewal-contracts.txt。
Watchman两个已审阅版本的协议、86字段、配置HTTP/缓存、异步运行时和事件采样已迁入Rust；
匿名配置200/304及实际WebView输入桥验证通过，见docs/core-watchman-runtime-contracts.txt与
docs/core-watchman-events-contracts.txt。原生Collection及真实WebView的38项设备/21项环境getter、
缓存/取消/页面重载已接通并对照，见docs/core-watchman-collection-contracts.txt；
正式音乐请求上下文和PC配置协商现已接入，真实设备/行为上报与本人歌单受保护读取均200；
中间件顺序、配置恢复、候选继承及证据见docs/core-pc-request-context-contracts.txt。
官方EAPI私密歌单创建已真实返回200并回读确认，隔离清理后原77个歌单完整恢复；
范围、原包/令牌审计与恢复入口见docs/core-pc-official-write-proof.txt，其他官方写路径继续验证。
后续完整歌单往返已通过：创建、元数据批处理、歌曲增删/排序/清空、歌单排序、删除共9次实际
写入全走EAPI且回读确认，原77歌单恢复。windows-local原生上下文已接入这组正式路由，
配置未就绪按“未提交”处理；详见docs/core-pc-playlist-official-contracts.txt。
两版本的原生指纹哈希、32项基础设备测量与插件/Canvas/WebGL已有实际窗口对照；
Windows声明适配和本机派生身份已通过真实账号读取，浏览器引擎差异仍有反证。
规则与边界见docs/core-watchman-measurements-contracts.txt及docs/windows-compatibility-research.txt。
正式桌面默认启动原生Watchman；显式CUMULUS_DEVICE_IDENTITY=windows-local同时选择原生请求与
采集上下文的Windows声明，仍保留真实WebKit渲染差异，不代表完整Windows模拟或全部403解决。
原生clientSign组合、持久ID、设备记录CBC/尾段变换、PFDW解析/导入导出及Linux事实采集已迁移；
账号候选保持同一上下文，真实只读、重开与正式桌面启动通过。显式启动选项及Windows边界见
docs/core-device-identity-contracts.txt；设备身份不等于易盾SDK或服务权限。
音效NCAE、材料片段解释和滤波/GEQ/低高音/九槽PEQ安全C接口已迁移并对照官方算法；
旋转/单声道多频段动态已接安全Rust接口，提供NCAE到旋转PCM的独立执行流程，
官方对照与状态/缓冲验证见docs/core-effects-spatial.txt。
官方M/3070参数组合及M/28内置回退参数已迁入独立Rust，涵盖逐歌/基础选择、增强关闭和有序合并；
446组官方对照及直接可执行工具见docs/core-effects-compose.txt。
音效目录/授权判断及账号隔离的参数材料库已接通，含新授权、暂停续传、完整性和删除恢复；
512组官方判断对照、6份真实材料及接口差异见docs/core-effects-library.txt。
音效授权登记/停止、服务端回收检查和未知结果恢复已迁移；一次非试用授权往返及重开通过，
见docs/core-effects-authorization.txt。设置/播放器生命周期接入、逐歌与设备材料继续。
完整DSP图、播放及音效UI接入继续，见docs/core-effects-ncae.txt和docs/core-effects-dsp.txt。
一起听已补自动结束/房间失效后的IM等待清理、共同试听解除和本机设置恢复，
退出未决只读核对、SDK失败及账号切换保护见docs/core-listen-together-lifecycle.txt。
一起听20008进度请求已接入当前原生时钟、忽略名单和未决/断线保护；官方对照及本地集成通过，
见docs/core-listen-together-progress-request.txt；不作为真实双端同步证明。
一起听已继续接入远端播放的原生原子应用、本机操作优先和持久失败恢复；本地故障/重启验证通过。
共享队列/共同权限、双方试听范围和退出清理已接入，并有本地集成与桌面夹具验证；
完整好友实时同步仍未开放，其余消息/RTC继续，见docs/core-listen-together-shared-queue.txt。
播客／有声书已接入主界面：收藏分类、详情、完整节目队列、单集评论与下载；
播放器保留节目身份并支持本地断点；收藏、单集点赞、续播设置已用真实账号往返验证。
验收与剩余分支见 docs/desktop-podcast-contracts.txt、docs/desktop-podcast-mutations-contracts.txt。
桌面已接入真实账号恢复、扫码弹窗、推荐、歌单分页、搜索和喜欢操作。
Rust core 已迁移协议、账号状态机、业务 API、队列与时间线；
Rust 已用原PC域会话读取真实账号资源，独立候选登录/续期/退出通过本地流程测试；
已迁移会员、歌词、NCM和本地库/CUE/目录监视，并分别验证服务或真实文件流程。
标签/封面、下载命名与账号离线记录已迁移；三格式写标签后的实际音频一致性已验证。
普通歌曲完整下载已串联授权、续传、三路调度、标签封面/LRC、可选NCM和离线登记，
两首真实FLAC与一个NCM输出通过独立校验；其他媒体分支仍在推进。
已完成文件的移动/改名/实体删除/移除记录与关联索引恢复已迁移，真实跨盘、进程退出和
原生播放器联动通过。目录配置、批量迁移、下载中工作文件/硬链接、逆序撤销已迁移，
真实跨盘12次进程退出恢复及原Range续传通过；单曲下载、设置与文件管理已接入桌面，
真实窗口已验证下载/暂停续传、本地播放、改名、目录迁移、删除与自建媒体清理。
独立Rust播放器已接入libmpv，真实账号CDN播放/控制/音质切换/暂停恢复及本地NCM播放通过。
桌面已接入原生播放条、完整歌单队列与歌词；真实窗口验收见 docs/desktop-player-contracts.txt。
歌单管理已接入新建、元信息/标签、增删与排序、清空/删除、收藏与操作核对；
隔离真实服务及桌面流程通过，其他边界见 docs/core-playlist-contracts.txt。
封面原生选图验证/裁剪、NOS上传、docId绑定和持久恢复已完成真实隔离往返，
原77个歌单及顺序恢复。正式选图/裁剪/缩放/保存、编辑草稿保留与操作记录恢复已接入，
正常及最小窗口实际选图、服务往返和进程重开通过，见docs/desktop-playlist-cover-contracts.txt。
云盘NOS上传、续期/重开、附件、编码/发布和账号回读已接入独立Rust用例；
新生成FLAC与PNG封面已用真实账号验证完整流程，隔离条目清理后原有49项资源恢复。
OSS签名/CRC64、三路分片、续传/核对与中止已接入Rust云盘父任务，官方对照及本地故障验证通过；
真实OSS通道仍缺服务分配条件，LRC/CUE保留原服务限制。
桌面云盘已接列表/容量/筛选、上传调度/目录选择、删除确认与核对、完整播放队列及专用歌词；
真实49首云盘列表/libmpv播放、原生选择上传/暂停重开继续/页面删除/基线恢复及最小窗口通过，
证据和剩余边界见 docs/desktop-cloud-contracts.txt。
云盘原文件下载已接入统一三路调度与桌面：保留原字节/标签、专用歌词、断点重开、离线播放和
完成文件管理；真实账号与窗口流程通过，详见 docs/core-cloud-download-contracts.txt。
云盘排序已补官方标题/歌手五态、其他列三态、中文自然比较与完整队列一致性；
官方JS/长列表与真实窗口对照见 docs/core-cloud-order-contracts.txt。
MV原生详情/收藏/下载和实际视频画面已迁移；真实账号240P/480P播放、暂停定位、
清晰度切换和续播恢复通过。正式MV页面、媒体焦点和二进制画面IPC已接入；
正常/最小窗口、全屏/小窗、MV下载与本地播放删除、隔离收藏往返已验证。
播放中改名/移动/目录迁移后的自动恢复及删除当前MV后停止已由真实窗口验证；
文件中断/回滚、旧选择与账号失效处理见docs/core-mv-local-files-contracts.txt。
通用评论已接歌曲/歌单/MV入口：最新/热评、翻页、定位、点赞/撤销与持久核对；
真实五类资源读取、隔离点赞往返和桌面交互通过，详细边界见docs/core-comments-contracts.txt。
MV资源点赞、关联歌曲权限/播放/同队列位置恢复，以及相似MV导航也已通过真实Rust与桌面验证；
相似接口的补充来源和恢复边界见docs/core-mv-social-contracts.txt。
本地音乐已接独立页面、目录监视/扫描进度、四类视图与完整队列；未登录也可播放本地音频，
支持CUE分轨、侧车歌词、NCM私有解码缓存和独立游客队列恢复。详见docs/desktop-local-library-contracts.txt。
评论纯文本发布/回复/本人删除、待确认操作及楼层已接入Rust/IPC/正式界面；
楼层真实两页27条及SQLite独立审计通过。公开写入使用合成服务验证，没有用真实账号发评；
最新窗口、恢复与剩余富文本边界见docs/core-comment-write-contracts.txt。
MV发现/云贝/其他视频等恢复分支仍继续，见docs/desktop-mv-contracts.txt。
本地音乐已接入官方元信息/批量线上匹配、持久任务、人工关联和线上评论入口，
真实账号与普通/最小桌面窗口已验证；关联保持本地文件播放。见docs/desktop-local-matching-contracts.txt。
播客/有声书原生列表、发现/历史读取、节目权限、播放与完整普通音频下载已迁移；免费节目实际
解码、暂停续传、离线与文件管理通过。收藏/声音点赞/续播开关有持久故障恢复测试；对应页面、
实际断点续播与其余边界仍在推进，详见docs/core-podcast-contracts.txt。
登录已接手机密码/SMS、邮箱、部分二次验证、安全问题、保存账号切换/移除、自动登录策略及退出；
生产UI/IPC/Rust配合合成服务的正常/最小窗口和重启流程已验证，未冒称真实新登录/短信通过。
账号移除的凭据清理、失败后显式继续和重启已通过真实窗口故障验证，见docs/desktop-saved-account-removal.txt。
滑块挑战已接入独立官方验证窗口；真人通过真实登录尚未验证，见 docs/desktop-captcha-contracts.txt。
设备验证、刷新宿主和剩余账号分支继续，见 docs/desktop-login-contracts.txt。
排行榜首页、数字专辑/单曲的日周总年榜、历史年榜、歌曲畅销指数榜已接 Rust 与桌面，
特殊榜单独立导航、专辑基础详情和完整专辑播放、账号缓存及失败重试已验证。
真实榜单100首队列和销售榜专辑3首队列实际播放通过；私人FM仍在推进，
来源/接口/23张窗口截图与边界见 docs/desktop-rankings-contracts.txt。
私人FM已迁入独立Rust领域：推荐缓存/权益身份、队列补充/历史裁剪、黑名单过滤、跳过上报
持久日志、减少/恢复推荐及只读核对。183组官方函数对照和15项新增Rust测试通过；
真实只读验证3首推荐、23首减少推荐歌曲、原因和缓存重开，修正服务count=0却返回歌曲的分支。
后续FM播放器协调及正式界面已接通，实际音频/EOF/单曲循环/原队列与重启恢复通过；
减少/恢复推荐、原因选择、歌手屏蔽/取消及丢回包只读核对已完成真实桌面往返，
详见docs/desktop-fm-feedback-contracts.txt。
熟悉歌曲和专区漫游已接真实服务、独立队列及桌面选择/播放：修正官方多首种子字段，
专区推荐补详情、trialMode=121、自然续曲与原队列恢复已验证。40个服务端专区采用前5项+
更多入口；卡片封面预览和播放按钮分离，缓存/分类失败回退/跨号拒绝已接通。
来源、接口和验收见docs/desktop-fm-modes-contracts.txt；其余FM分支继续，
不计作完整FM或完整目标完成。最新全量Rust与前端结果见docs/rust-core-validation.txt。
随心听FM已接官方场景目录、整组节目播放、未听选择、分类切换、自动播放保护与恢复；
播单详情、节目评论和下载弹窗已用真实账号验证，修正播单包含其他播客单集的兼容问题。
完整契约与阶段验收见docs/desktop-scene-fm-contracts.txt；原生回归和最终窗口记录见
docs/rust-core-validation.txt。完整播客发现页、封面取色及其余分支仍继续。
会员中心与历史日推已接入：五类真实权益、VIP推荐、会员榜单、数字专辑/歌单/有声书入口，
完整推荐队列按官方试听参数播放；日期切换、缓存/失败恢复、约12秒暂停重开均通过真实窗口验证。
来源与20张最终窗口证据见docs/desktop-member-contracts.txt；签到/礼券/动态开通等会员服务仍继续。
其余页面、剩余业务仍在迁移。逐项进度与验收边界见 docs/rust-core-migration.txt。

目录
  core/       独立 Rust 核心库；不依赖 Tauri、WebView、Linux D-Bus 或前端。
  desktop/    Tauri 桌面宿主；负责窗口、生命周期、命令桥接和平台适配。
  frontend/   Vue 3 界面；通过 CoreClient 调用业务，Tauri invoke 集中在 adapters/。
  docs/       新客户端的架构和开发说明。
  references/ 官方材料、第三方参考、恢复实现、历史报告、私有账号材料和验证证据。
  .build/     本地产物；由子目录 .gitignore 排除，不改动原根 .gitignore。

开发（从仓库根目录）
  npm --prefix frontend ci
  npm --prefix frontend run desktop:dev

检查与构建
  npm --prefix frontend test
  npm --prefix frontend run build
  cargo check --workspace --locked
  cargo test --workspace --locked
  npm --prefix frontend run desktop:build -- --debug --no-bundle

只开发界面：npm --prefix frontend run dev
浏览器预览不提供原生业务；开发界面可通过 CoreClient 注入受控测试实现，不伪装真实登录或播放。
核心库可独立构建：cargo check -p cumulus-core --locked。
需要 Rust stable、Node.js 22.12 或更高版本。各操作系统还需要 Tauri 对应的系统依赖：
  https://v2.tauri.app/start/prerequisites/

旧实现入口
  references/recovered-client/docs/product/13-client-core-progress.txt
  references/recovered-client/docs/product/14-client-core.txt
  references/recovered-client/README.txt

旧目标的历史结果是迁移依据，不表示新桌面客户端已经通过相同验收。
仓库原 LICENSE 保留。references/ 保持忽略，不随正式客户端构建或打包。
本轮检查结果与边界：docs/bootstrap-validation.txt。
Rust 迁移检查：docs/rust-core-validation.txt；来源索引：docs/migration-source-index.json。
完整桌面初版与官方风格UI目标：docs/full-desktop-goal.txt。

桌面业务契约与实际窗口验证：docs/ui-desktop-contracts.txt。
原生播放器与真实媒体验证：docs/core-playback-contracts.txt；本机需要libmpv，本地库探测使用ffprobe。
桌面播放/队列/歌词契约：docs/desktop-player-contracts.txt。
歌单核心/桌面契约与清理证明：docs/core-playlist-contracts.txt。
已完成文件管理、跨盘/进程恢复与播放联动：docs/core-completed-files-contracts.txt。
整目录迁移、工作文件/回滚与执行证明：docs/core-download-directory-contracts.txt。
桌面下载/文件/目录命令、公开数据、界面行为和剩余差异：docs/desktop-download-contracts.txt。
NOS对象上传协议、状态与验收边界：docs/core-nos-upload-contracts.txt。
OSS签名、CRC64、分片与恢复边界：docs/core-oss-upload-contracts.txt。
云盘完整上传、附件、编码、发布与账号回读：docs/core-cloud-upload-contracts.txt。
云盘桌面、删除恢复、上传调度、目录选择与播放器联动：docs/desktop-cloud-contracts.txt。
MV原生详情、收藏、下载/恢复与真实解码：docs/core-mv-contracts.txt。
MV视频状态机、真实流式画面、清晰度切换、恢复和桌面接入边界：docs/core-mv-video-contracts.txt。
MV正式页面、媒体互斥、二进制画面、实际窗口和可逆收藏/下载验收：docs/desktop-mv-contracts.txt。
评论接口、线程映射、点赞日志/缓存恢复、桌面入口与验证：docs/core-comments-contracts.txt。
MV资源点赞、关联歌曲/权限/播放恢复、相似MV及来源差异：docs/core-mv-social-contracts.txt。
本机打包前端产物：.build/target/debug/cumulus。
默认使用应用数据目录；测试既有授权PC会话可显式设置CUMULUS_SESSION_DIR，
另设CUMULUS_DATA_DIR隔离桌面状态。绝不自动加载references中的登录材料。
前端状态测试使用Node.js 24内置TypeScript与test runner；普通构建仍支持上述Node最低版本。

一起听单账号Rust房间/队列/播放指令/心跳/IM与RTC令牌读取已完成真实隔离REST验证，
丢回包只读恢复及房间清理通过；现已接房间页面、实际播放条/队列、退出偏好恢复及关闭失败核对，
正常和最小窗口主流程有真实验证，详见docs/desktop-listen-together-contracts.txt。
官方Linux NIM SDK已通过Rust原生登录、入房、接收入房通知、清理和同进程重开验证；
桌面已接入消息连接状态、重新连接、收到消息重置心跳及等待SDK清理后退出；
正常窗口与最小窗口关闭失败核对已实测。播放业务包应用与RTC仍继续，不能把入房通知当作双端同步通过。
原生SDK来源、FFI边界及桌面证据见docs/core-listen-together-native-im.txt、docs/desktop-listen-together-im.txt。

官方一起听业务消息来源已从nim_wrapper.dll确认为nim_talk的msg_attach；已接入普通/批量接收与房间过滤，
播放/队列/权益消息的完整应用仍在推进，详见docs/core-listen-together-business-messages.txt。
