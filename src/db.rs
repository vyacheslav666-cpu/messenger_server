use crate::{error::{AppError, AppResult}, models::{ChatListItem, ChatMessage, UserResponse}};
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
             created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
             deleted_at DATETIME DEFAULT NULL
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
         CREATE TABLE IF NOT EXISTS direct_chat_reads (
             user_id INTEGER NOT NULL,
             target_id INTEGER NOT NULL,
             last_read_message_id INTEGER NOT NULL DEFAULT 0,
             updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
             PRIMARY KEY (user_id, target_id),
             FOREIGN KEY(user_id) REFERENCES users(id),
             FOREIGN KEY(target_id) REFERENCES users(id)
         );
         CREATE TABLE IF NOT EXISTS user_blocks (
             blocker_id INTEGER NOT NULL,
             blocked_id INTEGER NOT NULL,
             created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
             PRIMARY KEY (blocker_id, blocked_id),
             FOREIGN KEY(blocker_id) REFERENCES users(id),
             FOREIGN KEY(blocked_id) REFERENCES users(id)
         );
         CREATE INDEX IF NOT EXISTS idx_messages_pair ON messages(sender_id, receiver_id, id);
         CREATE INDEX IF NOT EXISTS idx_messages_receiver ON messages(receiver_id, id);
         CREATE INDEX IF NOT EXISTS idx_user_blocks_blocked ON user_blocks(blocked_id);"
    )
}

pub fn create_user(conn: &Connection, login: &str, password: &str) -> AppResult<()> {
    let login = login.trim();
    let password = password.trim();

    if login.len() < 3 {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Логин минимум 3 символа"));
    }
    if login.len() > 32 || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Логин: 3-32 символа, латиница/цифры/_/-"));
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
    let mut stmt = conn.prepare("SELECT id, login, password_hash FROM users WHERE login = ?1 AND deleted_at IS NULL")
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

pub fn search_users(conn: &Connection, login: &str, current_user_id: i64) -> AppResult<Vec<UserResponse>> {
    let pattern = format!("%{}%", login.trim());
    let mut stmt = conn.prepare(
        "SELECT id, login FROM users
         WHERE login LIKE ?1 AND id != ?2 AND deleted_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM user_blocks b WHERE b.blocker_id = ?2 AND b.blocked_id = users.id)
           AND NOT EXISTS (SELECT 1 FROM user_blocks b WHERE b.blocker_id = users.id AND b.blocked_id = ?2)
         ORDER BY login LIMIT 20"
    ).map_err(|e| AppError::internal(e.to_string()))?;

    let rows = stmt.query_map(params![pattern, current_user_id], |row| {
        Ok(UserResponse { user_id: row.get(0)?, login: row.get(1)? })
    }).map_err(|e| AppError::internal(e.to_string()))?;

    Ok(rows.filter_map(Result::ok).collect())
}

pub fn conversation(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, sender_id, receiver_id, text, timestamp FROM messages
         WHERE (sender_id = ?1 AND receiver_id = ?2) OR (sender_id = ?2 AND receiver_id = ?1)
         ORDER BY id ASC"
    ).map_err(|e| AppError::internal(e.to_string()))?;

    let rows = stmt.query_map(params![user_id, target_id], |row| {
        Ok(ChatMessage {
            id: row.get(0)?,
            sender_id: row.get(1)?,
            receiver_id: row.get(2)?,
            text: row.get(3)?,
            timestamp: row.get(4)?,
        })
    }).map_err(|e| AppError::internal(e.to_string()))?;

    let messages: Vec<ChatMessage> = rows.filter_map(Result::ok).collect();
    if let Some(last) = messages.last() {
        mark_direct_chat_read(conn, user_id, target_id, last.id)?;
    }
    Ok(messages)
}

pub fn mark_direct_chat_read(conn: &Connection, user_id: i64, target_id: i64, last_read_message_id: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO direct_chat_reads (user_id, target_id, last_read_message_id, updated_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
         ON CONFLICT(user_id, target_id) DO UPDATE SET
             last_read_message_id = MAX(last_read_message_id, excluded.last_read_message_id),
             updated_at = CURRENT_TIMESTAMP",
        params![user_id, target_id, last_read_message_id],
    ).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

pub fn active_chats(conn: &Connection, user_id: i64) -> AppResult<Vec<ChatListItem>> {
    let mut stmt = conn.prepare(
        "WITH peers AS (
             SELECT CASE WHEN sender_id = ?1 THEN receiver_id ELSE sender_id END AS peer_id,
                    MAX(id) AS last_message_id,
                    MAX(timestamp) AS last_message_at
             FROM messages
             WHERE sender_id = ?1 OR receiver_id = ?1
             GROUP BY peer_id
         )
         SELECT u.id,
                u.login,
                COALESCE((
                    SELECT COUNT(*) FROM messages m
                    LEFT JOIN direct_chat_reads r ON r.user_id = ?1 AND r.target_id = u.id
                    WHERE m.sender_id = u.id
                      AND m.receiver_id = ?1
                      AND m.id > COALESCE(r.last_read_message_id, 0)
                ), 0) AS unread_count,
                peers.last_message_at
         FROM peers
         JOIN users u ON u.id = peers.peer_id
         WHERE u.deleted_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM user_blocks b WHERE b.blocker_id = ?1 AND b.blocked_id = u.id)
           AND NOT EXISTS (SELECT 1 FROM user_blocks b WHERE b.blocker_id = u.id AND b.blocked_id = ?1)
         ORDER BY peers.last_message_id DESC"
    ).map_err(|e| AppError::internal(e.to_string()))?;

    let rows = stmt.query_map(params![user_id], |row| {
        Ok(ChatListItem {
            user_id: row.get(0)?,
            login: row.get(1)?,
            unread_count: row.get(2)?,
            last_message_at: row.get(3)?,
        })
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

    let receiver_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1 AND deleted_at IS NULL)",
        params![receiver_id],
        |row| row.get(0),
    ).map_err(|e| AppError::internal(e.to_string()))?;
    if !receiver_exists {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Получатель не найден"));
    }

    if is_blocked_either_way(conn, sender_id, receiver_id)? {
        return Err(AppError::new(StatusCode::FORBIDDEN, "Переписка заблокирована"));
    }

    conn.execute(
        "INSERT INTO messages (sender_id, receiver_id, text) VALUES (?1, ?2, ?3)",
        params![sender_id, receiver_id, text],
    ).map_err(|e| AppError::internal(e.to_string()))?;

    let id = conn.last_insert_rowid();
    conn.query_row(
        "SELECT id, sender_id, receiver_id, text, timestamp FROM messages WHERE id = ?1",
        params![id],
        |row| Ok(ChatMessage {
            id: row.get(0)?,
            sender_id: row.get(1)?,
            receiver_id: row.get(2)?,
            text: row.get(3)?,
            timestamp: row.get(4)?,
        })
    ).map_err(|e| AppError::internal(e.to_string()))
}


pub fn is_blocked_either_way(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<bool> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM user_blocks
             WHERE (blocker_id = ?1 AND blocked_id = ?2)
                OR (blocker_id = ?2 AND blocked_id = ?1)
         )",
        params![user_id, target_id],
        |row| row.get(0),
    ).map_err(|e| AppError::internal(e.to_string()))
}

pub fn block_user(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<()> {
    if user_id <= 0 || target_id <= 0 || user_id == target_id {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Неверная цель блокировки"));
    }
    let target_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1 AND deleted_at IS NULL)",
        params![target_id],
        |row| row.get(0),
    ).map_err(|e| AppError::internal(e.to_string()))?;
    if !target_exists {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Пользователь не найден"));
    }
    conn.execute(
        "INSERT OR IGNORE INTO user_blocks (blocker_id, blocked_id) VALUES (?1, ?2)",
        params![user_id, target_id],
    ).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

pub fn unblock_user(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<()> {
    conn.execute(
        "DELETE FROM user_blocks WHERE blocker_id = ?1 AND blocked_id = ?2",
        params![user_id, target_id],
    ).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

pub fn delete_account(conn: &Connection, user_id: i64, password: &str) -> AppResult<()> {
    let saved_hash: String = conn.query_row(
        "SELECT password_hash FROM users WHERE id = ?1 AND deleted_at IS NULL",
        params![user_id],
        |row| row.get(0),
    ).map_err(|_| AppError::new(StatusCode::UNAUTHORIZED, "Аккаунт не найден"))?;

    match verify(password.trim(), &saved_hash) {
        Ok(true) => {}
        _ => return Err(AppError::new(StatusCode::UNAUTHORIZED, "Неверный пароль")),
    }

    conn.execute("DELETE FROM messages WHERE sender_id = ?1 OR receiver_id = ?1", params![user_id])
        .map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute("DELETE FROM direct_chat_reads WHERE user_id = ?1 OR target_id = ?1", params![user_id])
        .map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute("DELETE FROM user_blocks WHERE blocker_id = ?1 OR blocked_id = ?1", params![user_id])
        .map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute(
        "UPDATE users SET login = 'deleted_' || id, password_hash = '', deleted_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![user_id],
    ).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}
