use crate::Error;
use serde::Serialize;

/// Stable, deliberately bounded public errors. No transport strings, response
/// bodies, endpoints, cookies, filesystem paths or challenge tokens.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientError {
    pub code: &'static str,
    pub message: &'static str,
    pub retryable: bool,
}

impl From<Error> for ClientError {
    fn from(error: Error) -> Self {
        let (code, message, retryable) = if error.is_service_code(301) {
            ("session_expired", "登录已失效，请重新登录", false)
        } else {
            match error.root() {
                Error::LoginDeferred => ("login_check", "请求未通过登录校验，请稍后重试", true),
                Error::State("CAPTCHA timed out") => {
                    ("captcha_timeout", "验证码已超时，请重新打开验证", true)
                }
                Error::State("CAPTCHA unavailable") => (
                    "captcha_unavailable",
                    "验证码暂时无法加载，请稍后重试",
                    true,
                ),
                Error::State("CAPTCHA is already pending") => {
                    ("captcha_pending", "请先完成或关闭已打开的验证码", false)
                }
                Error::RequestNotSent(_) => (
                    "request_not_sent",
                    "请求准备未完成，修改尚未提交，请稍后重试",
                    true,
                ),
                Error::InvalidInput("comment must contain 1 to 1000 UTF-16 units") => {
                    ("comment_text", "请输入不超过 1000 字符的评论内容", false)
                }
                Error::InvalidInput("only own comments may be deleted") => {
                    ("comment_owner", "只能删除自己发表的评论", false)
                }
                Error::State("comment target is unavailable" | "comment is no longer readable") => {
                    (
                        "comment_unavailable",
                        "这条评论暂时无法读取，可能已删除或被屏蔽",
                        false,
                    )
                }
                Error::Service { endpoint, .. }
                    if matches!(
                        endpoint.as_str(),
                        "/api/resource/comments/add" | "/api/v1/resource/comments/reply"
                    ) && error.is_service_code(480) =>
                {
                    ("comment_phone", "发布评论前需要绑定手机号", false)
                }
                Error::Service { endpoint, .. }
                    if matches!(
                        endpoint.as_str(),
                        "/api/resource/comments/add" | "/api/v1/resource/comments/reply"
                    ) && error.is_service_code(250) =>
                {
                    (
                        "comment_rejected",
                        "评论未通过服务校验，请修改后重试",
                        false,
                    )
                }
                Error::InvalidInput(
                    "cover input must contain 1 to 5MiB bytes" | "cover exceeds 5MiB",
                ) => ("cover_size", "请选择不超过 5 MB 的图片", false),
                Error::InvalidInput("cover resolution must be at least 320x320") => {
                    ("cover_dimensions", "请选择宽高至少 320 像素的图片", false)
                }
                Error::InvalidInput("cover crop is outside the image") => {
                    ("cover_crop", "裁剪范围超出图片，请重新调整", false)
                }
                Error::Media(
                    "invalid cover format"
                    | "cover must be JPEG, PNG, BMP or GIF"
                    | "invalid cover image"
                    | "invalid image orientation",
                ) => (
                    "cover_format",
                    "无法读取这张图片，请选择有效的 JPEG、PNG、BMP 或 GIF",
                    false,
                ),
                Error::Media("cover dimensions exceed 16 megapixels") => (
                    "cover_dimensions",
                    "图片像素过大，请选择不超过 1600 万像素的图片",
                    false,
                ),
                Error::StaleResponse | Error::Cancelled => {
                    ("stale", "操作已取消或账号已切换", false)
                }
                Error::InvalidInput(_) => ("invalid_input", "输入内容不符合要求", false),
                Error::QueueChanged => ("snapshot_changed", "列表已更新，请刷新后重试", true),
                Error::State("like recovery changed") => (
                    "like_changed",
                    "喜欢状态或待核对记录已变化，请取消本次确认并重新核对",
                    true,
                ),
                Error::State("reconcile like state before accepting it") => {
                    ("like_reconcile", "请先核对喜欢状态，再决定是否保留", false)
                }
                Error::State("account settings changed") => (
                    "settings_changed",
                    "账号设置已发生变化，请查看最新状态后重新选择",
                    true,
                ),
                Error::State("relay setting changed") => (
                    "relay_setting_changed",
                    "设备开关已发生变化，请检查最新状态后重新选择",
                    true,
                ),
                Error::State("read back relay setting before acceptance") => (
                    "relay_setting_reconcile",
                    "请先核对当前设备开关，再决定是否保留",
                    false,
                ),
                Error::State(
                    "native relay device identity unavailable"
                    | "relay setting unavailable"
                    | "relay setting experiment unavailable"
                    | "read relay setting first",
                ) => (
                    "relay_setting_unavailable",
                    "当前设备设置暂不可用，请重新检查跨端续播状态",
                    true,
                ),
                Error::State("reconcile account settings before acceptance") => {
                    ("settings_reconcile", "请先核对这次设置修改的结果", false)
                }
                Error::Response("setting is absent or unsupported") => (
                    "setting_unavailable",
                    "服务端未返回可修改的设置值，请刷新后重试",
                    true,
                ),
                Error::State("FM queue requires its coordinator") => {
                    ("fm_queue", "FM队列随推荐更新，请使用上一首或下一首", false)
                }
                Error::State("FM queue update requires recovery") => (
                    "fm_recovery",
                    "FM切歌尚未完成，请在私人FM页重试继续播放",
                    true,
                ),
                Error::State("leave together before entering FM") => {
                    ("fm_together", "请先退出一起听，再进入私人FM", false)
                }
                Error::State("leave FM before entering together") => {
                    ("together_fm", "请先返回原播放队列，再进入一起听", false)
                }
                Error::State("equalizer settings changed") => {
                    ("equalizer_changed", "均衡器设置已更新，请重新调整", true)
                }
                Error::State("playback settings changed") => {
                    ("settings_changed", "播放设置已更新，请重新读取后调整", true)
                }
                Error::State("playback settings damaged") => (
                    "settings_damaged",
                    "播放设置损坏，请在设置中恢复默认值；也可以手动播放",
                    false,
                ),
                Error::State(
                    "playback settings revision damaged" | "playback settings revision exhausted",
                ) => (
                    "settings_unwritable",
                    "本机播放设置记录无法安全更新，已保留现有值；仍可手动播放",
                    false,
                ),
                Error::State("finish login before entering together") => {
                    ("login_pending", "请先完成或取消登录，再进入一起听", false)
                }
                Error::State("start a new login attempt") => (
                    "login_restart",
                    "请重新开始登录；结果未确认时可先核对登录状态",
                    false,
                ),
                Error::State("verification resend cooldown is active") => {
                    ("login_cooldown", "验证码发送仍在冷却中，请稍后再试", false)
                }
                Error::Response("saved account identity changed") => (
                    "saved_account_changed",
                    "该账号的登录身份已变化，请重新登录",
                    false,
                ),
                Error::State("cloud capacity exhausted") => {
                    ("cloud_full", "云盘空间已满，请清理部分歌曲后再上传", false)
                }
                Error::State("together requires normal playback speed") => {
                    ("together_speed", "一起听期间请使用正常播放速度", false)
                }
                Error::State("realtime together connection is not ready") => (
                    "together_connection",
                    "好友实时连接尚未就绪，暂不能接管共同播放",
                    false,
                ),
                Error::State("restore previous together playback settings first") => {
                    ("together_restore", "请先恢复上次一起听的播放设置", false)
                }
                Error::State("together room is ending") => (
                    "together_ending",
                    "一起听正在结束，请核对退出结果或重试退出",
                    false,
                ),
                Error::State(
                    "IM cleanup unresolved"
                    | "IM cleanup unresolved; restart the process"
                    | "IM supervisor failed"
                    | "IM supervisor missing",
                ) => (
                    "together_cleanup",
                    "消息连接清理失败，请重新启动客户端后恢复播放设置",
                    false,
                ),
                Error::State("together room returned during cleanup") => (
                    "together_room_returned",
                    "服务端仍显示原房间，请刷新核对后退出",
                    true,
                ),
                Error::State("leave together before switching accounts") => (
                    "together_account",
                    "请先退出一起听并恢复播放设置，再切换账号",
                    false,
                ),
                Error::State("cannot remove current account") => (
                    "current_account",
                    "请先退出或切换账号，再移除这条登录记录",
                    false,
                ),
                Error::State("saved account removal has started") => (
                    "account_removing",
                    "此账号正在移除，请先完成本地凭据清理",
                    false,
                ),
                Error::State("FM artist change rejected") => (
                    "artist_rejected",
                    "歌曲已减少推荐，歌手屏蔽失败，可在反馈状态中重试",
                    true,
                ),
                Error::State("reload FM reasons") => {
                    ("reload_reasons", "请重新打开减少推荐窗口", true)
                }
                Error::MutationUncertain => {
                    ("write_uncertain", "操作结果尚未确认，请先刷新核对", false)
                }
                Error::MutationBusy => ("busy", "此操作正在进行，请稍后重试", true),
                Error::CredentialChanged => {
                    ("profile_changed", "登录资料已更新，请重启客户端", false)
                }
                Error::Transport(_) | Error::HttpStatus { .. } => {
                    ("network", "网络请求失败，请稍后重试", true)
                }
                Error::Service { .. } => ("service_rejected", "服务暂未允许此操作", false),
                Error::Playback {
                    operation: "library" | "ABI",
                    ..
                } => (
                    "audio_unavailable",
                    "无法加载音频组件，请检查 libmpv 安装",
                    true,
                ),
                Error::Playback { .. } => ("playback_failed", "音频加载或播放失败，请重试", true),
                Error::Storage(_) | Error::Io(_) => {
                    ("storage", "无法读写本地数据，请检查空间与权限", true)
                }
                _ => ("unavailable", "暂时无法完成此操作", true),
            }
        };
        Self {
            code,
            message,
            retryable,
        }
    }
}
