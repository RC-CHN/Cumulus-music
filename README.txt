Cumulus Music

跨平台社区音乐客户端，采用 Tauri 2、Rust 和 Vue 3 / TypeScript。
支持 Linux x86_64 和 Windows x86_64，不支持 macOS。

安装与版本
  安装包：https://github.com/RC-CHN/Cumulus-music/releases
  0.2.0 发行说明与已知边界：docs/releases/0.2.0.txt
  实际已发布版本以 GitHub Release 为准。

  Linux 提供 .deb 和 tar.gz，需要 WebKitGTK 4.1（>= 2.42）、libmpv2
  和 FFmpeg 等系统依赖。Windows 提供 x64 NSIS 安装包和便携 zip，需要
  Microsoft Edge WebView2；安装包未配置签名证书。
  下载后可使用同一 Release 的 SHA256SUMS 校验文件。

功能与边界
  登录与账号恢复、推荐与搜索、歌单与专辑、播客与有声书、本地音乐、
  下载与云盘、队列与歌词、音质与音效、迷你播放器和平台集成持续完善。
  当前功能、官方来源、实测证据及缺口统一记录在 docs/desktop-feature-matrix.md。
  开发分支的进度不等于某个已发布版本的能力；请同时核对该版本发行说明。

  账号、会员权益和风控由上游服务决定。微信与 QQ 登录尚未开放；
  网易专有宿主的客服、日志上传和官方更新器不在产品范围。
  一起听双账号同步、语音及部分专有音效仍有验证缺口。
  合成媒体、null 输出或 CI 编译通过不代表真实声卡、实服或全部业务验证通过。

项目结构
  core/       独立 Rust 业务核心，不依赖 Tauri、WebView 或 Python 子进程。
  desktop/    Tauri 宿主、窗口、平台适配与类型化 IPC。
  frontend/   Vue 界面，通过 CoreClient 调用业务；Tauri invoke 集中在 adapters/。
  scripts/ci/ 构建、合成回归、产物来源核对与发布门禁。
  docs/       架构、业务契约、官方来源索引与验证记录。
  references/ 忽略的官方材料、研究归档与私有验证证据，不提交或打包。
  .build/     忽略的本地构建和验证产物，不提交。

开发
  使用 rust-toolchain.toml 和 .node-version 固定的 Rust / Node 版本及锁文件。
  安装对应平台的 Tauri 系统依赖：
    https://v2.tauri.app/start/prerequisites/
  原生播放需要 libmpv；本地媒体探测使用 ffprobe。

  npm --prefix frontend ci
  npm --prefix frontend run desktop:dev

  只开发界面：npm --prefix frontend run dev
  浏览器预览不提供原生业务；受控 CoreClient 夹具不代表真实登录或播放。
  核心库可独立检查：cargo check -p cumulus-core --locked
  本地调试构建：npm --prefix frontend run desktop:build -- --debug --no-bundle

必需检查（从仓库根目录执行）
  npm --prefix frontend ci
  npm --prefix frontend run format:check
  npm --prefix frontend test
  npm --prefix frontend run build
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --features cumulus-desktop/custom-protocol --locked -- -D warnings
  cargo test --workspace --all-targets --features cumulus-desktop/custom-protocol --locked
  python3 scripts/ci/source_audit.py
  python3 scripts/ci/release_guard.py
  python3 -m unittest discover -s scripts/ci -p 'test_*.py'

  Linux、Windows 的原生测试和发行构建、前端检查使用独立 CI runner。
  真实 WebView 和安装包回归也必须通过 CI gate；本地检查不替代这些验证。
  研究脚本使用的解包材料或真实账号不属于公开 CI。

维护与发布
  工作约定和强制发布门槛：AGENTS.md
  CI、打包、发布及失败处理：docs/releasing.txt
  功能与证据索引：docs/desktop-feature-matrix.md
  近期交付记录：docs/desktop-delivery-checkpoints.txt
  界面验收边界：docs/ui-ux-validation.txt
  原生播放契约：docs/core-playback-contracts.txt
  本地文件打开与系统关联：docs/default-player-contracts.txt
  官方恢复来源：docs/migration-source-index.json

  发布前先完成实现、版本文件、发行说明及完整 CI，再创建唯一的空 release
  commit。只有该提交自身完整 CI 成功后才创建标签，从同次 CI 发布已验证产物。
  不移动已发布标签或覆盖附件；修复使用下一版本。仓库原 LICENSE 保留。
