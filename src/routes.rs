use crate::{
    db,
    error::{AppError, AppResult},
    models::{
        AuthRequest, AuthResponse, BlockRequest, ChatListItem, CreateGroupRequest,
        CreateGroupResponse, DeleteAccountRequest, HistoryQuery, MessageSearchQuery, SearchQuery, UserResponse,
    },
    state::AppState,
    websocket,
};
use axum::{extract::{Query, State, ws::WebSocketUpgrade}, http::StatusCode, response::IntoResponse, Json};
use std::{collections::HashMap, sync::Arc};

pub async fn register_handler(State(state): State<Arc<AppState>>, Json(payload): Json<AuthRequest>) -> AppResult<StatusCode> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::create_user(&db, &payload.login, &payload.password)?;
    Ok(StatusCode::CREATED)
}

pub async fn login_handler(State(state): State<Arc<AppState>>, Json(payload): Json<AuthRequest>) -> AppResult<Json<AuthResponse>> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    let (user_id, login) = db::verify_user(&db, &payload.login, &payload.password)?;
    Ok(Json(AuthResponse { user_id, login }))
}

pub async fn search_handler(State(state): State<Arc<AppState>>, Query(query): Query<SearchQuery>) -> AppResult<Json<Vec<UserResponse>>> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    let current_user_id = query.user_id.unwrap_or(0);
    Ok(Json(db::search_users(&db, &query.login, current_user_id)?))
}

pub async fn history_handler(State(state): State<Arc<AppState>>, Query(query): Query<HistoryQuery>) -> AppResult<Json<Vec<crate::models::ChatMessage>>> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    if let Some(chat_id) = query.chat_id {
        return Ok(Json(db::group_conversation(&db, query.user_id, chat_id)?));
    }
    let target_id = query.target_id.ok_or_else(|| AppError::new(StatusCode::BAD_REQUEST, "Нужен target_id или chat_id"))?;
    if db::is_blocked_either_way(&db, query.user_id, target_id)? {
        return Err(AppError::new(StatusCode::FORBIDDEN, "Переписка заблокирована"));
    }
    Ok(Json(db::conversation(&db, query.user_id, target_id)?))
}


pub async fn message_search_handler(State(state): State<Arc<AppState>>, Query(query): Query<MessageSearchQuery>) -> AppResult<Json<Vec<crate::models::MessageSearchResult>>> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    Ok(Json(db::search_messages(&db, query.user_id, &query.q, query.target_id, query.chat_id)?))
}

pub async fn active_chats_handler(State(state): State<Arc<AppState>>, Query(params): Query<HashMap<String, String>>) -> AppResult<Json<Vec<ChatListItem>>> {
    let user_id = params.get("user_id").and_then(|id| id.parse::<i64>().ok()).ok_or_else(|| AppError::new(StatusCode::BAD_REQUEST, "Нужен user_id"))?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    Ok(Json(db::active_chats(&db, user_id)?))
}

pub async fn create_group_handler(State(state): State<Arc<AppState>>, Json(payload): Json<CreateGroupRequest>) -> AppResult<Json<CreateGroupResponse>> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    Ok(Json(db::create_group(&db, payload.user_id, &payload.title, &payload.member_logins)?))
}

pub async fn block_handler(State(state): State<Arc<AppState>>, Json(payload): Json<BlockRequest>) -> AppResult<StatusCode> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::block_user(&db, payload.user_id, payload.target_id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unblock_handler(State(state): State<Arc<AppState>>, Json(payload): Json<BlockRequest>) -> AppResult<StatusCode> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::unblock_user(&db, payload.user_id, payload.target_id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_account_handler(State(state): State<Arc<AppState>>, Json(payload): Json<DeleteAccountRequest>) -> AppResult<StatusCode> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::delete_account(&db, payload.user_id, &payload.password)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn ws_route_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>, Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let user_id = params.get("user_id").and_then(|id| id.parse::<i64>().ok()).unwrap_or(0);
    ws.on_upgrade(move |socket| websocket::handle_user_socket(socket, state, user_id))
}
