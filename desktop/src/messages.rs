use cumulus_core::client::{
    ClientError, MusicClient,
    dto::AccountData,
    messages::{MessageCounts, MessagePage, MessageQuery},
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
