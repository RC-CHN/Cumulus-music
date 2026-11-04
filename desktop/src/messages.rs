use cumulus_core::client::{
    ClientError, MusicClient,
    dto::AccountData,
    messages::{
        MessageCounts, MessageDeleteAction, MessageDeleteOutcome, MessagePage,
        MessagePendingDeletion, MessageQuery, MessageReplyAction, MessageReplyView,
    },
};

#[tauri::command]
pub async fn message_counts(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<MessageCounts>, ClientError> {
    client.message_counts(&epoch).await.map_err(Into::into)
}

#[tauri::command]
pub async fn message_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: MessageQuery,
) -> Result<AccountData<MessagePage>, ClientError> {
    client.message_page(&epoch, query).await.map_err(Into::into)
}

#[tauri::command]
pub async fn pending_message_deletions(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<AccountData<Vec<MessagePendingDeletion>>, ClientError> {
    client
        .pending_message_deletions(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn delete_message(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    action: MessageDeleteAction,
) -> Result<AccountData<MessageDeleteOutcome>, ClientError> {
    client
        .delete_message(&epoch, &receipt, action)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn message_reply(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: MessageReplyAction,
) -> Result<AccountData<MessageReplyView>, ClientError> {
    client
        .message_reply(&epoch, action)
        .await
        .map_err(Into::into)
}
