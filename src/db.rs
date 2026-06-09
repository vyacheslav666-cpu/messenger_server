use crate::{error::{AppError, AppResult}, models::{ChatMessage, UserResponse}};
use axum::http::StatusCode;
use bcrypt::{hash, verify, DEFAULT_COST};
use rusqlite::{params, Connection};

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS users (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             login TEXT NOT NULL UNIQUE,
             password_hash TEXT NOT NULL,
             created_at DATETIME DEFAULT CURRENT_TIMESTAMP
         );
         CREATE TABLE IF NOT EXISTS messages (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             sender_id INTEGER NOT NULL,
             receiver_id INTEGER NOT NULL,
             text TEXT NOT NULL,
             timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
             FOREIGN KEY(sender_id) REFERENCES users(id),
             FOREIGN KEY(receiver_id) REFERENCES users(id)
         );
         CREATE INDEX IF NOT EXISTS idx_messages_pair ON messages(sender_id, receiver_id, id);"
    )
}

pub fn create_user(conn: &Connection, login: &str, password: &str) -> AppResult<()> {
    let login = login.trim();
    let password = password.trim();

    if login.len() < 3 {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Логин минимум 3 символа"));
    }
    if password.len() < 4 {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Пароль минимум 4 символа"));
    }

    let hashed = hash(password, DEFAULT_COST).map_err(|_| AppError::internal("Ошибка хэширования пароля"))?;

    match conn.execute(
        "INSERT INTO users (login, password_hash) VALUES (?1, ?2)",
        params![login, hashed],
    ) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("UNIQUE") => Err(AppError::new(StatusCode::CONFLICT, "Логин занят")),
        Err(e) => Err(AppError::internal(e.to_string())),
    }
}

pub fn verify_user(conn: &Connection, login: &str, password: &str) -> AppResult<(i64, String)> {
    let login = login.trim();
    let mut stmt = conn.prepare("SELECT id, login, password_hash FROM users WHERE login = ?1")
        .map_err(|e| AppError::internal(e.to_string()))?;

    let row = stmt.query_row(params![login], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    });

    match row {
        Ok((id, saved_login, saved_hash)) => match verify(password, &saved_hash) {
            Ok(true) => Ok((id, saved_login)),
            _ => Err(AppError::new(StatusCode::UNAUTHORIZED, "Неверные данные")),
        },
        Err(_) => Err(AppError::new(StatusCode::UNAUTHORIZED, "Неверные данные")),
    }
}

pub fn search_users(conn: &Connection, login: &str) -> AppResult<Vec<UserResponse>> {
    let pattern = format!("%{}%", login.trim());
    let mut stmt = conn.prepare("SELECT id, login FROM users WHERE login LIKE ?1 ORDER BY login LIMIT 20")
        .map_err(|e| AppError::internal(e.to_string()))?;

    let rows = stmt.query_map(params![pattern], |row| {
        Ok(UserResponse { user_id: row.get(0)?, login: row.get(1)? })
    }).map_err(|e| AppError::internal(e.to_string()))?;

    Ok(rows.filter_map(Result::ok).collect())
}

pub fn conversation(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT sender_id, receiver_id, text, timestamp FROM messages
         WHERE (sender_id = ?1 AND receiver_id = ?2) OR (sender_id = ?2 AND receiver_id = ?1)
         ORDER BY id ASC"
    ).map_err(|e| AppError::internal(e.to_string()))?;

    let rows = stmt.query_map(params![user_id, target_id], |row| {
        Ok(ChatMessage {
            sender_id: row.get(0)?,
            receiver_id: row.get(1)?,
            text: row.get(2)?,
            timestamp: row.get(3)?,
        })
    }).map_err(|e| AppError::internal(e.to_string()))?;

    Ok(rows.filter_map(Result::ok).collect())
}

pub fn active_chats(conn: &Connection, user_id: i64) -> AppResult<Vec<UserResponse>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT u.id, u.login
         FROM users u
         JOIN messages m ON m.sender_id = u.id OR m.receiver_id = u.id
         WHERE (m.sender_id = ?1 OR m.receiver_id = ?1) AND u.id != ?1
         ORDER BY u.login"
    ).map_err(|e| AppError::internal(e.to_string()))?;

    let rows = stmt.query_map(params![user_id], |row| {
        Ok(UserResponse { user_id: row.get(0)?, login: row.get(1)? })
    }).map_err(|e| AppError::internal(e.to_string()))?;

    Ok(rows.filter_map(Result::ok).collect())
}

pub fn save_message(conn: &Connection, sender_id: i64, receiver_id: i64, text: &str) -> AppResult<ChatMessage> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Пустое сообщение"));
    }
    if text.len() > 4000 {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Сообщение слишком длинное"));
    }

    conn.execute(
        "INSERT INTO messages (sender_id, receiver_id, text) VALUES (?1, ?2, ?3)",
        params![sender_id, receiver_id, text],
    ).map_err(|e| AppError::internal(e.to_string()))?;

    let id = conn.last_insert_rowid();
    conn.query_row(
        "SELECT sender_id, receiver_id, text, timestamp FROM messages WHERE id = ?1",
        params![id],
        |row| Ok(ChatMessage {
            sender_id: row.get(0)?,
            receiver_id: row.get(1)?,
            text: row.get(2)?,
            timestamp: row.get(3)?,
        })
    ).map_err(|e| AppError::internal(e.to_string()))
}
