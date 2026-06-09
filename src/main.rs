use axum::{
    routing::{get, post},
    Json, Router, http::{Method, StatusCode},
    extract::{State, Query},
};
use tower_http::cors::{Any, CorsLayer};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use bcrypt::{hash, verify, DEFAULT_COST};

// Состояние сервера с базой данных
struct AppState {
    db: Mutex<Connection>,
}

// Пакет данных для авторизации
#[derive(Deserialize)]
struct AuthRequest {
    login: String,
    password_hash: String,
}

// Ответ при успешном входе
#[derive(Serialize)]
struct AuthResponse {
    user_id: i64,
    login: String,
}

// Параметры для GET-запроса поиска людей (например, /search?login=slava)
#[derive(Deserialize)]
struct SearchQuery {
    login: String,
}

// Формат ответа списка пользователей для фронтенда
#[derive(Serialize)]
struct UserSearchResponse {
    user_id: i64,
    login: String,
}

#[tokio::main]
async fn main() {
    let conn = Connection::open("chat.db").expect("Не удалось открыть chat.db");
    init_db(&conn);

    let shared_state = Arc::new(AppState {
        db: Mutex::new(conn),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(vec![Method::GET, Method::POST])
        .allow_headers(Any);

    // Добавляем роут /search для поиска контактов
    let app = Router::new()
        .route("/", get(|| async { "Бэкенд мессенджера работает!" }))
        .route("/register", post(register_handler))
        .route("/login", post(login_handler))
        .route("/search", get(search_handler)) // Регистрация поиска
        .with_state(shared_state)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Сервер мессенджера запущен на http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ХЭНДЛЕР ПОИСКА ЛЮДЕЙ В БАЗЕ
async fn search_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<UserSearchResponse>>, (StatusCode, String)> {
    let search_pattern = format!("%{}%", query.login.trim()); // Формируем строку для LIKE (%запрос%)

    let db = state.db.lock().unwrap();

    // Вытаскиваем юзеров, чей логин похож на поисковый запрос
    let mut stmt = db.prepare("SELECT id, login FROM users WHERE login LIKE ?1 LIMIT 20")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let user_rows = stmt.query_map(params![search_pattern], |row| {
        Ok(UserSearchResponse {
            user_id: row.get(0)?,
            login: row.get(1)?,
        })
    }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut found_users = Vec::new();
    for user_res in user_rows {
        if let Ok(user) = user_res {
            found_users.push(user);
        }
    }

    Ok(Json(found_users))
}

// ХЭНДЛЕР РЕГИСТРАЦИИ
async fn register_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AuthRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let login = payload.login.trim();
    let password = payload.password_hash.trim();

    if login.is_empty() || password.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Логин и пароль не могут быть пустыми".to_string()));
    }

    let hashed_password = hash(password, DEFAULT_COST)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Ошибка шифрования пароля".to_string()))?;

    let db = state.db.lock().unwrap();
    
    match db.execute(
        "INSERT INTO users (login, password_hash) VALUES (?1, ?2)",
        params![login, hashed_password],
    ) {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(e) => {
            if e.to_string().contains("UNIQUE constraint failed") {
                Err((StatusCode::CONFLICT, "Пользователь с таким логином уже существует".to_string()))
            } else {
                Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Ошибка БД: {}", e)))
            }
        }
    }
}

// ХЭНДЛЕР ВХОДА
async fn login_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, String)> {
    let login = payload.login.trim();
    let password = payload.password_hash.trim();

    let db = state.db.lock().unwrap();

    let mut stmt = db.prepare("SELECT id, password_hash FROM users WHERE login = ?1")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let user_row = stmt.query_row(params![login], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    });

    match user_row {
        Ok((user_id, saved_hash)) => {
            match verify(password, &saved_hash) {
                Ok(true) => Ok(Json(AuthResponse { user_id, login: login.to_string() })),
                _ => Err((StatusCode::UNAUTHORIZED, "Неверный логин или пароль".to_string())),
            }
        }
        Err(_) => Err((StatusCode::UNAUTHORIZED, "Неверный логин или пароль".to_string())),
    }
}

fn init_db(conn: &Connection) {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            login TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL
        )",
        [],
    ).expect("Ошибка создания таблицы users");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS contacts (
            user_id INTEGER NOT NULL,
            contact_id INTEGER NOT NULL,
            PRIMARY KEY (user_id, contact_id),
            FOREIGN KEY (user_id) REFERENCES users(id),
            FOREIGN KEY (contact_id) REFERENCES users(id)
        )",
        [],
    ).expect("Ошибка создания таблицы contacts");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sender_id INTEGER NOT NULL,
            receiver_id INTEGER NOT NULL,
            text TEXT NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (sender_id) REFERENCES users(id),
            FOREIGN KEY (receiver_id) REFERENCES users(id)
        )",
        [],
    ).expect("Ошибка создания таблицы messages");
}