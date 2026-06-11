use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct AuthRequest {
    pub login: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub user_id: i64,
    pub login: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub login: String,
    pub user_id: Option<i64>,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub user_id: i64,
    pub login: String,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub user_id: i64,
    pub target_id: i64,
}

#[derive(Serialize)]
pub struct ChatListItem {
    pub user_id: i64,
    pub login: String,
    pub unread_count: i64,
    pub last_message_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: i64,
    pub sender_id: i64,
    pub receiver_id: i64,
    pub text: String,
    pub timestamp: String,
}

#[derive(Deserialize)]
pub struct IncomingWsMessage {
    pub receiver_id: i64,
    pub text: String,
}

#[derive(Deserialize)]
pub struct BlockRequest {
    pub user_id: i64,
    pub target_id: i64,
}

#[derive(Deserialize)]
pub struct DeleteAccountRequest {
    pub user_id: i64,
    pub password: String,
}
