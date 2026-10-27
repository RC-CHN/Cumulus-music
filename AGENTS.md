# Cumulus Music 工作约定

## 范围和分层

- 当前支持目标是 Linux x86_64 和 Windows x86_64；不承诺 macOS 支持。
- `core/` 是独立 Rust 业务核心，不依赖 Tauri、WebView 或 Python 子进程；C DSP 只能通过明确的安全 FFI 边界接入。
- `desktop/` 负责 Tauri、窗口、平台适配和 IPC；`frontend/` 只通过类型化接口调用核心，不能持有 Cookie、签名密钥或完整授权 URL。
- 官方恢复证据优先；仅在证据不足时参考其他客户端，并记录来源。未知、受外部条件限制和未经实测的分支必须明确说明。

## 提交和数据

- 按职责分批提交，使用说明实际变化的 Conventional Commit 标题；提交前检查暂存区。
- 不提交 `references/`、`.build/`、真实账号/设备会话、下载音乐、截图中的私人信息、构建产物和依赖缓存。
- 测试使用合成数据和冻结的无私人信息对照向量。需要解包材料或真实账号的研究脚本不属于公开 CI 的自动执行范围。
- 不将编译成功、接口占位或历史 Python 测试称为 Rust/桌面功能验证通过。
- 不自动支付、公开发内容、向他人发信/邀请或修改已有账号资源。线上验证必须有当次或历史明确授权。

## 验证

- 使用仓库固定的 Rust/Node 版本和锁文件。Rust：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --features cumulus-desktop/custom-protocol --locked -- -D warnings`、`cargo test --workspace --all-targets --features cumulus-desktop/custom-protocol --locked`。
- 前端：`npm --prefix frontend ci`，再运行 `format:check`、`test` 和 `build`。
- Linux、Windows 的测试及发行构建分配到独立 runner 并行执行；前端检查独立执行。所有必需 job 汇总到 `CI gate`，不能通过忽略失败或 `continue-on-error` 发布。
- 原生解码测试使用合成音频和 null 输出，不代表真实声卡听感；没有第二账号时不宣称一起听双端通过。
- 桌面原生播放必须在真实 GUI 初始化后验证；Linux 使用非 C 系统 locale，确认仅数字 locale 调整为 C、界面语言设置保留。单独的 core 解码测试不能覆盖 GTK 初始化影响。
- 界面滚动与动画需在真实 WebView 验证；列表保持有界渲染，动画优先使用 transform/opacity 并尊重减少动态效果。GPU 兼容设置只作用于本进程且保留用户覆盖；虚拟 CI runner 的帧率不能当成实机刷新率证明。

## 版本发布（强制）

- 版本标签格式为 `vX.Y.Z`，必须直接指向标题严格等于 `chore:release X.Y.Z` 的提交。
- 同一个版本在仓库历史中只能有一条 release commit。不得通过再次提交同版本 release commit、移动已发布标签、覆盖已发布附件来修补版本；发布后修复使用下一版本。
- 先完成所有实现、版本文件、发行说明及打包配置，并让 CI 通过，再创建唯一的 release commit。release commit 使用空提交标记已验证的内容树，不夹带其他修改。
- **只有标签目标提交自身的完整 CI 成功才允许创建标签和发布版本。** 上一提交成功、局部 job 成功或本地测试通过都不够。
- CD 必须核对 CI workflow、仓库、main 分支、精确提交 SHA、运行结果、版本文件和 release commit 唯一性，再从该 CI 运行下载产物；不能用另一次构建冒充已验证产物。
- Linux 和 Windows 独立 runner 生成产物；统一发布 job 在全部检查成功后生成校验和并发布 GitHub Release。
- 发布操作须有用户授权；用户明确要求发布指定版本时无需重复确认。缺权限、签名凭据或 runner 条件时如实报告，不绕过检查。
