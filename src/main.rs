use axum::{
    routing::{get, post},
    Json, Router, http::{Method, StatusCode},
    extract::{State, Query, ws::{Message, WebSocket, WebSocketUpgrade}},
    response::IntoResponse,
};
use tower_http::cors::{Any, CorsLayer};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use bcrypt::{hash, verify, DEFAULT_COST};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

// Хранилище активных WebSocket подключений (ID юзера -> канал отправки сообщений)
type ActiveConnections = Arc<Mutex<HashMap<i64, mpsc::UnboundedSender<String>>>>;

struct AppState {
    db: Mutex<Connection>,
    conns: ActiveConnections,
}

#[derive(Deserialize)]
struct AuthRequest {
    login: String,
    password_hash: String,
}

#[derive(Serialize)]
struct AuthResponse {
    user_id: i64,
    login: String,
}

#[derive(Deserialize)]
struct SearchQuery {
    login: String,
}

#[derive(Serialize)]
struct UserSearchResponse {
    user_id: i64,
    login: String,
}

// Параметры для получения истории переписки
#[derive(Deserialize)]
struct HistoryQuery {
    user_id: i64,
    target_id: i64,
}

// Структура сообщения для выдачи истории и гоняния по WebSockets
#[derive(Serialize, Deserialize, Clone)]
struct ChatMessage {
    sender_id: i64,
    receiver_id: i64,
    text: String,
    timestamp: String,
}

#[tokio::main]
async fn main() {
    let conn = Connection::open("chat.db").expect("Не удалось открыть chat.db");
    init_db(&conn);

    let shared_state = Arc::new(AppState {
        db: Mutex::new(conn),
        conns: Arc::new(Mutex::new(HashMap::new())),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(vec![Method::GET, Method::POST])
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { "Бэкенд мессенджера работает!" }))
        .route("/register", post(register_handler))
        .route("/login", post(login_handler))
        .route("/search", get(search_handler))
        .route("/messages", get(history_handler)) // Роут истории
        .route("/ws", get(ws_route_handler))       // Роут сокетов
        .with_state(shared_state)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Сервер мессенджера запущен на http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ХЭНДЛЕР ЗАГРУЗКИ ИСТОРИИ СООБЩЕНИЙ
async fn history_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<ChatMessage>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    
    // Достаем сообщения, где отправитель и получатель — это наши два юзера (в обе стороны)
    let mut stmt = db.prepare(
        "SELECT sender_id, receiver_id, text, timestamp FROM messages 
         WHERE (sender_id = ?1 AND receiver_id = ?2) OR (sender_id = ?2 AND receiver_id = ?1)
         ORDER BY id ASC"
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let msg_rows = stmt.query_map(params![query.user_id, query.target_id], |row| {
        Ok(ChatMessage {
            sender_id: row.get(0)?,
            receiver_id: row.get(1)?,
            text: row.get(2)?,
            timestamp: row.get(3)?,
        })
    }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut history = Vec::new();
    for msg in msg_rows {
        if let Ok(m) = msg { history.push(m); }
    }
    Ok(Json(history))
}

// ТОЧКА ВХОДА ДЛЯ ВЕБ-СОКЕТА
async fn ws_route_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // Клиент при подключении передает свой id в ссылке: ws://.../ws?user_id=1
    let user_id = params.get("user_id")
        .and_then(|id| id.parse::<i64>().ok())
        .unwrap_or(0);

    ws.on_upgrade(move |socket| handle_user_socket(socket, state, user_id))
}

// РАБОТА С СОКЕТОМ ЮЗЕРА
async fn handle_user_socket(socket: WebSocket, state: Arc<AppState>, user_id: i64) {
    if user_id == 0 { return; }

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Запоминаем сокет пользователя в глобальном списке сети
    {
        let mut conns = state.conns.lock().unwrap();
        conns.insert(user_id, tx);
    }

    // Задача отправки сообщений в браузер этому юзеру
    let mut send_task = tokio::spawn(async move {
        while let Some(msg_text) = rx.recv().await {
            if ws_sender.send(Message::Text(msg_text.into())).await.is_err() {
                break;
            }
        }
    });

    // Задача чтения сообщений от этого юзера
    let conns_clone = state.conns.clone();
    let state_clone = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = ws_receiver.next().await {
            if let Ok(msg) = serde_json::from_str::<ChatMessage>(&text) {
                
                // 1. Сохраняем сообщение в базу данных
                {
                    let db = state_clone.db.lock().unwrap();
                    let _ = db.execute(
                        "INSERT INTO messages (sender_id, receiver_id, text) VALUES (?1, ?2, ?3)",
                        params![msg.sender_id, msg.receiver_id, msg.text],
                    );
                }

                // 2. Пытаемся отправить получателю напрямую, если он онлайн
                let conns = conns_clone.lock().unwrap();
                if let Some(receiver_tx) = conns.get(&msg.receiver_id) {
                    let _ = receiver_tx.send(text);
                }
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    // Клиент отключился — удаляем его из списка онлайн-подключений
    let mut conns = state.conns.lock().unwrap();
    conns.remove(&user_id);
}

// ХЭНДЛЕР ПОИСКА
async fn search_handler(State(state): State<Arc<AppState>>, Query(query): Query<SearchQuery>) -> Result<Json<Vec<UserSearchResponse>>, (StatusCode, String)> {
    let search_pattern = format!("%{}%", query.login.trim());
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare("SELECT id, login FROM users WHERE login LIKE ?1 LIMIT 20").map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let user_rows = stmt.query_map(params![search_pattern], |row| { Ok(UserSearchResponse { user_id: row.get(0)?, login: row.get(1)? }) }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut found_users = Vec::new();
    for user_res in user_rows { if let Ok(user) = user_res { found_users.push(user); } }
    Ok(Json(found_users))
}

// ХЭНДЛЕР РЕГИСТРАЦИИ
async fn register_handler(State(state): State<Arc<AppState>>, Json(payload): Json<AuthRequest>) -> Result<StatusCode, (StatusCode, String)> {
    let login = payload.login.trim(); let password = payload.password_hash.trim();
    if login.is_empty() || password.is_empty() { return Err((StatusCode::BAD_REQUEST, "Пустые поля".to_string())); }
    let hashed_password = hash(password, DEFAULT_COST).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Ошибка хэша".to_string()))?;
    let db = state.db.lock().unwrap();
    match db.execute("INSERT INTO users (login, password_hash) VALUES (?1, ?2)", params![login, hashed_password]) {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(e) => { if e.to_string().contains("UNIQUE") { Err((StatusCode::CONFLICT, "Логин занят".to_string())) } else { Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())) } }
    }
}

// ХЭНДЛЕР ВХОДА
async fn login_handler(State(state): State<Arc<AppState>>, Json(payload): Json<AuthRequest>) -> Result<Json<AuthResponse>, (StatusCode, String)> {
    let login = payload.login.trim(); let password = payload.password_hash.trim();
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare("SELECT id, password_hash FROM users WHERE login = ?1").map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let user_row = stmt.query_row(params![login], |row| { Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)) });
    match user_row {
        Ok((user_id, saved_hash)) => {
            match verify(password, &saved_hash) {
                Ok(true) => Ok(Json(AuthResponse { user_id, login: login.to_string() })),
                _ => Err((StatusCode::UNAUTHORIZED, "Неверные данные".to_string())),
            }
        }
        Err(_) => Err((StatusCode::UNAUTHORIZED, "Неверные данные".to_string())),
    }
}

fn init_db(conn: &Connection) {
    conn.execute("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY AUTOINCREMENT, login TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL)", []).unwrap();
    conn.execute("CREATE TABLE IF NOT EXISTS contacts (user_id INTEGER NOT NULL, contact_id INTEGER NOT NULL, PRIMARY KEY (user_id, contact_id))", []).unwrap();
    conn.execute("CREATE TABLE IF NOT EXISTS messages (id INTEGER PRIMARY KEY AUTOINCREMENT, sender_id INTEGER NOT NULL, receiver_id INTEGER NOT NULL, text TEXT NOT NULL, timestamp DATETIME DEFAULT CURRENT_TIMESTAMP)", []).unwrap();
}