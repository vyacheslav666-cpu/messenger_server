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
use axum::{
    extract::{ws::WebSocketUpgrade, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::{collections::HashMap, sync::Arc};

pub async fn register_handler(State(state): State<Arc<AppState>>, Json(payload): Json<AuthRequest>) -> AppResult<StatusCode> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::create_user(&db, &payload.login, &payload.password)?;
    Ok(StatusCode::CREATED)
}

pub async fn login_handler(State(state): State<Arc<AppState>>, Json(payload): Json<AuthRequest>) -> AppResult<Json<AuthResponse>> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    let (user_id, login) = db::verify_user(&db, &payload.login, &payload.password)?;
    let token = db::create_session(&db, user_id)?;
    Ok(Json(AuthResponse { user_id, login, token }))
}

pub async fn search_handler(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(query): Query<SearchQuery>) -> AppResult<Json<Vec<UserResponse>>> {
    let current_user_id = authenticated_user_id(&state, &headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    Ok(Json(db::search_users(&db, &query.login, current_user_id)?))
}

pub async fn history_handler(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(query): Query<HistoryQuery>) -> AppResult<Json<Vec<crate::models::ChatMessage>>> {
    let user_id = authenticated_user_id(&state, &headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    if let Some(chat_id) = query.chat_id {
        return Ok(Json(db::group_conversation(&db, user_id, chat_id)?));
    }
    let target_id = query.target_id.ok_or_else(|| AppError::new(StatusCode::BAD_REQUEST, "Нужен target_id или chat_id"))?;
    if db::is_blocked_either_way(&db, user_id, target_id)? {
        return Err(AppError::new(StatusCode::FORBIDDEN, "Переписка заблокирована"));
    }
    Ok(Json(db::conversation(&db, user_id, target_id)?))
}

pub async fn message_search_handler(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(query): Query<MessageSearchQuery>) -> AppResult<Json<Vec<crate::models::MessageSearchResult>>> {
    let user_id = authenticated_user_id(&state, &headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    Ok(Json(db::search_messages(&db, user_id, &query.q, query.target_id, query.chat_id)?))
}

pub async fn active_chats_handler(State(state): State<Arc<AppState>>, headers: HeaderMap) -> AppResult<Json<Vec<ChatListItem>>> {
    let user_id = authenticated_user_id(&state, &headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    Ok(Json(db::active_chats(&db, user_id)?))
}

pub async fn create_group_handler(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(payload): Json<CreateGroupRequest>) -> AppResult<Json<CreateGroupResponse>> {
    let user_id = authenticated_user_id(&state, &headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    Ok(Json(db::create_group(&db, user_id, &payload.title, &payload.member_logins)?))
}

pub async fn block_handler(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(payload): Json<BlockRequest>) -> AppResult<StatusCode> {
    let user_id = authenticated_user_id(&state, &headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::block_user(&db, user_id, payload.target_id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unblock_handler(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(payload): Json<BlockRequest>) -> AppResult<StatusCode> {
    let user_id = authenticated_user_id(&state, &headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::unblock_user(&db, user_id, payload.target_id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_account_handler(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(payload): Json<DeleteAccountRequest>) -> AppResult<StatusCode> {
    let user_id = authenticated_user_id(&state, &headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::delete_account(&db, user_id, &payload.password)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn ws_route_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>, Query(params): Query<HashMap<String, String>>) -> Response {
    let token = match params.get("token") {
        Some(token) => token,
        None => return AppError::new(StatusCode::UNAUTHORIZED, "Нужна авторизация").into_response(),
    };

    let user_id = match websocket_user_id(&state, token) {
        Ok(user_id) => user_id,
        Err(err) => return err.into_response(),
    };

    ws.on_upgrade(move |socket| websocket::handle_user_socket(socket, state, user_id)).into_response()
}

fn authenticated_user_id(state: &AppState, headers: &HeaderMap) -> AppResult<i64> {
    let token = bearer_token(headers)?;
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::authenticate_token(&db, token)
}

fn websocket_user_id(state: &AppState, token: &str) -> AppResult<i64> {
    let db = state.db.lock().map_err(|_| AppError::internal("DB lock poisoned"))?;
    db::authenticate_token(&db, token)
}

fn bearer_token(headers: &HeaderMap) -> AppResult<&str> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Нужна авторизация"))?;

    value
        .strip_prefix("Bearer ")
        .filter(|token| !token.trim().is_empty())
        .map(str::trim)
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Нужна авторизация"))
}
