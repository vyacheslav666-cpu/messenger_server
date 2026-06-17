use crate::{error::{AppError, AppResult}, models::{ChatListItem, ChatMessage, CreateGroupResponse, MessageSearchResult, UserResponse}};
use axum::http::StatusCode;
use bcrypt::{hash, verify, DEFAULT_COST};
use rusqlite::{params, Connection, OptionalExtension};

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
             receiver_id INTEGER,
             chat_id INTEGER,
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
             PRIMARY KEY (user_id, target_id)
         );
         CREATE TABLE IF NOT EXISTS user_blocks (
             blocker_id INTEGER NOT NULL,
             blocked_id INTEGER NOT NULL,
             created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
             PRIMARY KEY (blocker_id, blocked_id)
         );
         CREATE TABLE IF NOT EXISTS chats (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             chat_type TEXT NOT NULL DEFAULT 'group',
             title TEXT NOT NULL,
             created_by INTEGER NOT NULL,
             created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
             deleted_at DATETIME DEFAULT NULL
         );
         CREATE TABLE IF NOT EXISTS chat_members (
             chat_id INTEGER NOT NULL,
             user_id INTEGER NOT NULL,
             joined_at DATETIME DEFAULT CURRENT_TIMESTAMP,
             left_at DATETIME DEFAULT NULL,
             PRIMARY KEY (chat_id, user_id)
         );
         CREATE TABLE IF NOT EXISTS group_chat_reads (
             user_id INTEGER NOT NULL,
             chat_id INTEGER NOT NULL,
             last_read_message_id INTEGER NOT NULL DEFAULT 0,
             updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
             PRIMARY KEY (user_id, chat_id)
         );
         CREATE INDEX IF NOT EXISTS idx_messages_pair ON messages(sender_id, receiver_id, id);
         CREATE INDEX IF NOT EXISTS idx_messages_group ON messages(chat_id, id);
         CREATE INDEX IF NOT EXISTS idx_messages_receiver ON messages(receiver_id, id);
         CREATE INDEX IF NOT EXISTS idx_user_blocks_blocked ON user_blocks(blocked_id);
         CREATE INDEX IF NOT EXISTS idx_chat_members_user ON chat_members(user_id, chat_id);"
    )?;

    // Миграция старой БД v0.3: если messages уже была без chat_id/receiver_id nullable.
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN chat_id INTEGER", []);
    Ok(())
}

pub fn create_user(conn: &Connection, login: &str, password: &str) -> AppResult<()> {
    let login = login.trim();
    let password = password.trim();
    if login.len() < 3 { return Err(AppError::new(StatusCode::BAD_REQUEST, "Логин минимум 3 символа")); }
    if login.len() > 32 || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Логин: 3-32 символа, латиница/цифры/_/-"));
    }
    if password.len() < 4 { return Err(AppError::new(StatusCode::BAD_REQUEST, "Пароль минимум 4 символа")); }
    let hashed = hash(password, DEFAULT_COST).map_err(|_| AppError::internal("Ошибка хэширования пароля"))?;
    match conn.execute("INSERT INTO users (login, password_hash) VALUES (?1, ?2)", params![login, hashed]) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("UNIQUE") => Err(AppError::new(StatusCode::CONFLICT, "Логин занят")),
        Err(e) => Err(AppError::internal(e.to_string())),
    }
}

pub fn verify_user(conn: &Connection, login: &str, password: &str) -> AppResult<(i64, String)> {
    let mut stmt = conn.prepare("SELECT id, login, password_hash FROM users WHERE login = ?1 AND deleted_at IS NULL")
        .map_err(|e| AppError::internal(e.to_string()))?;
    let row = stmt.query_row(params![login.trim()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)));
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
    let rows = stmt.query_map(params![pattern, current_user_id], |row| Ok(UserResponse { user_id: row.get(0)?, login: row.get(1)? }))
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn conversation(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.sender_id, m.receiver_id, m.chat_id, m.text, m.timestamp, u.login
         FROM messages m JOIN users u ON u.id = m.sender_id
         WHERE m.chat_id IS NULL AND ((m.sender_id = ?1 AND m.receiver_id = ?2) OR (m.sender_id = ?2 AND m.receiver_id = ?1))
         ORDER BY m.id ASC"
    ).map_err(|e| AppError::internal(e.to_string()))?;
    let rows = stmt.query_map(params![user_id, target_id], row_to_msg).map_err(|e| AppError::internal(e.to_string()))?;
    let messages: Vec<ChatMessage> = rows.filter_map(Result::ok).collect();
    if let Some(last) = messages.last() { mark_direct_chat_read(conn, user_id, target_id, last.id)?; }
    Ok(messages)
}

pub fn group_conversation(conn: &Connection, user_id: i64, chat_id: i64) -> AppResult<Vec<ChatMessage>> {
    if !is_group_member(conn, user_id, chat_id)? {
        return Err(AppError::new(StatusCode::FORBIDDEN, "Нет доступа к группе"));
    }
    let mut stmt = conn.prepare(
        "SELECT m.id, m.sender_id, m.receiver_id, m.chat_id, m.text, m.timestamp, u.login
         FROM messages m JOIN users u ON u.id = m.sender_id
         WHERE m.chat_id = ?1
         ORDER BY m.id ASC"
    ).map_err(|e| AppError::internal(e.to_string()))?;
    let rows = stmt.query_map(params![chat_id], row_to_msg).map_err(|e| AppError::internal(e.to_string()))?;
    let messages: Vec<ChatMessage> = rows.filter_map(Result::ok).collect();
    if let Some(last) = messages.last() { mark_group_chat_read(conn, user_id, chat_id, last.id)?; }
    Ok(messages)
}

fn row_to_msg(row: &rusqlite::Row) -> rusqlite::Result<ChatMessage> {
    Ok(ChatMessage {
        id: row.get(0)?,
        sender_id: row.get(1)?,
        receiver_id: row.get(2)?,
        chat_id: row.get(3)?,
        text: row.get(4)?,
        timestamp: row.get(5)?,
        sender_login: row.get(6)?,
    })
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

pub fn mark_group_chat_read(conn: &Connection, user_id: i64, chat_id: i64, last_read_message_id: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO group_chat_reads (user_id, chat_id, last_read_message_id, updated_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
         ON CONFLICT(user_id, chat_id) DO UPDATE SET
             last_read_message_id = MAX(last_read_message_id, excluded.last_read_message_id),
             updated_at = CURRENT_TIMESTAMP",
        params![user_id, chat_id, last_read_message_id],
    ).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

pub fn active_chats(conn: &Connection, user_id: i64) -> AppResult<Vec<ChatListItem>> {
    let mut out = Vec::new();

    let mut direct_stmt = conn.prepare(
        "WITH peers AS (
             SELECT CASE WHEN sender_id = ?1 THEN receiver_id ELSE sender_id END AS peer_id,
                    MAX(id) AS last_message_id,
                    MAX(timestamp) AS last_message_at
             FROM messages
             WHERE chat_id IS NULL AND (sender_id = ?1 OR receiver_id = ?1)
             GROUP BY peer_id
         )
         SELECT u.id, u.login,
                COALESCE((
                    SELECT COUNT(*) FROM messages m
                    LEFT JOIN direct_chat_reads r ON r.user_id = ?1 AND r.target_id = u.id
                    WHERE m.chat_id IS NULL AND m.sender_id = u.id AND m.receiver_id = ?1 AND m.id > COALESCE(r.last_read_message_id, 0)
                ), 0) AS unread_count,
                peers.last_message_at
         FROM peers JOIN users u ON u.id = peers.peer_id
         WHERE u.deleted_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM user_blocks b WHERE b.blocker_id = ?1 AND b.blocked_id = u.id)
           AND NOT EXISTS (SELECT 1 FROM user_blocks b WHERE b.blocker_id = u.id AND b.blocked_id = ?1)
         ORDER BY peers.last_message_id DESC"
    ).map_err(|e| AppError::internal(e.to_string()))?;
    let rows = direct_stmt.query_map(params![user_id], |row| Ok(ChatListItem {
        chat_type: "direct".to_string(),
        chat_id: None,
        user_id: Some(row.get(0)?),
        title: row.get(1)?,
        unread_count: row.get(2)?,
        last_message_at: row.get(3)?,
    })).map_err(|e| AppError::internal(e.to_string()))?;
    out.extend(rows.filter_map(Result::ok));

    let mut group_stmt = conn.prepare(
        "SELECT c.id, c.title,
                COALESCE((
                    SELECT COUNT(*) FROM messages m
                    LEFT JOIN group_chat_reads r ON r.user_id = ?1 AND r.chat_id = c.id
                    WHERE m.chat_id = c.id AND m.sender_id != ?1 AND m.id > COALESCE(r.last_read_message_id, 0)
                ), 0) AS unread_count,
                (SELECT MAX(timestamp) FROM messages WHERE chat_id = c.id) AS last_message_at
         FROM chats c
         JOIN chat_members cm ON cm.chat_id = c.id AND cm.user_id = ?1 AND cm.left_at IS NULL
         WHERE c.deleted_at IS NULL
         ORDER BY COALESCE((SELECT MAX(id) FROM messages WHERE chat_id = c.id), c.id) DESC"
    ).map_err(|e| AppError::internal(e.to_string()))?;
    let rows = group_stmt.query_map(params![user_id], |row| Ok(ChatListItem {
        chat_type: "group".to_string(),
        chat_id: Some(row.get(0)?),
        user_id: None,
        title: row.get(1)?,
        unread_count: row.get(2)?,
        last_message_at: row.get(3)?,
    })).map_err(|e| AppError::internal(e.to_string()))?;
    out.extend(rows.filter_map(Result::ok));

    out.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at));
    Ok(out)
}

pub fn save_direct_message(conn: &Connection, sender_id: i64, receiver_id: i64, text: &str) -> AppResult<ChatMessage> {
    validate_message(text)?;
    let receiver_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1 AND deleted_at IS NULL)", params![receiver_id], |row| row.get(0),
    ).map_err(|e| AppError::internal(e.to_string()))?;
    if !receiver_exists { return Err(AppError::new(StatusCode::BAD_REQUEST, "Получатель не найден")); }
    if is_blocked_either_way(conn, sender_id, receiver_id)? { return Err(AppError::new(StatusCode::FORBIDDEN, "Переписка заблокирована")); }
    conn.execute("INSERT INTO messages (sender_id, receiver_id, chat_id, text) VALUES (?1, ?2, NULL, ?3)", params![sender_id, receiver_id, text.trim()])
        .map_err(|e| AppError::internal(e.to_string()))?;
    get_message(conn, conn.last_insert_rowid())
}

pub fn save_group_message(conn: &Connection, sender_id: i64, chat_id: i64, text: &str) -> AppResult<ChatMessage> {
    validate_message(text)?;
    if !is_group_member(conn, sender_id, chat_id)? { return Err(AppError::new(StatusCode::FORBIDDEN, "Нет доступа к группе")); }
    conn.execute("INSERT INTO messages (sender_id, receiver_id, chat_id, text) VALUES (?1, NULL, ?2, ?3)", params![sender_id, chat_id, text.trim()])
        .map_err(|e| AppError::internal(e.to_string()))?;
    get_message(conn, conn.last_insert_rowid())
}

fn validate_message(text: &str) -> AppResult<()> {
    let text = text.trim();
    if text.is_empty() { return Err(AppError::new(StatusCode::BAD_REQUEST, "Пустое сообщение")); }
    if text.len() > 4000 { return Err(AppError::new(StatusCode::BAD_REQUEST, "Сообщение слишком длинное")); }
    Ok(())
}

fn get_message(conn: &Connection, id: i64) -> AppResult<ChatMessage> {
    conn.query_row(
        "SELECT m.id, m.sender_id, m.receiver_id, m.chat_id, m.text, m.timestamp, u.login
         FROM messages m JOIN users u ON u.id = m.sender_id WHERE m.id = ?1",
        params![id], row_to_msg
    ).map_err(|e| AppError::internal(e.to_string()))
}

pub fn is_blocked_either_way(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id = ?1 AND blocked_id = ?2) OR (blocker_id = ?2 AND blocked_id = ?1))",
        params![user_id, target_id], |row| row.get(0),
    ).map_err(|e| AppError::internal(e.to_string()))
}

pub fn block_user(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<()> {
    if user_id <= 0 || target_id <= 0 || user_id == target_id { return Err(AppError::new(StatusCode::BAD_REQUEST, "Неверная цель блокировки")); }
    conn.execute("INSERT OR IGNORE INTO user_blocks (blocker_id, blocked_id) VALUES (?1, ?2)", params![user_id, target_id])
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

pub fn unblock_user(conn: &Connection, user_id: i64, target_id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM user_blocks WHERE blocker_id = ?1 AND blocked_id = ?2", params![user_id, target_id])
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

pub fn delete_account(conn: &Connection, user_id: i64, password: &str) -> AppResult<()> {
    let saved_hash: String = conn.query_row(
        "SELECT password_hash FROM users WHERE id = ?1 AND deleted_at IS NULL", params![user_id], |row| row.get(0),
    ).map_err(|_| AppError::new(StatusCode::UNAUTHORIZED, "Аккаунт не найден"))?;
    match verify(password.trim(), &saved_hash) { Ok(true) => {}, _ => return Err(AppError::new(StatusCode::UNAUTHORIZED, "Неверный пароль")), }
    conn.execute("DELETE FROM messages WHERE sender_id = ?1 OR receiver_id = ?1", params![user_id]).map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute("DELETE FROM direct_chat_reads WHERE user_id = ?1 OR target_id = ?1", params![user_id]).map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute("DELETE FROM group_chat_reads WHERE user_id = ?1", params![user_id]).map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute("UPDATE chat_members SET left_at = CURRENT_TIMESTAMP WHERE user_id = ?1", params![user_id]).map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute("DELETE FROM user_blocks WHERE blocker_id = ?1 OR blocked_id = ?1", params![user_id]).map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute("UPDATE users SET login = 'deleted_' || id, password_hash = '', deleted_at = CURRENT_TIMESTAMP WHERE id = ?1", params![user_id])
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

pub fn create_group(conn: &Connection, creator_id: i64, title: &str, member_logins: &[String]) -> AppResult<CreateGroupResponse> {
    let title = title.trim();
    if title.len() < 2 || title.len() > 64 { return Err(AppError::new(StatusCode::BAD_REQUEST, "Название группы: 2-64 символа")); }

    conn.execute("INSERT INTO chats (chat_type, title, created_by) VALUES ('group', ?1, ?2)", params![title, creator_id])
        .map_err(|e| AppError::internal(e.to_string()))?;
    let chat_id = conn.last_insert_rowid();
    conn.execute("INSERT OR IGNORE INTO chat_members (chat_id, user_id) VALUES (?1, ?2)", params![chat_id, creator_id])
        .map_err(|e| AppError::internal(e.to_string()))?;

    for raw in member_logins.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let member_id: Option<i64> = conn.query_row(
            "SELECT id FROM users WHERE login = ?1 AND deleted_at IS NULL", params![raw], |row| row.get(0),
        ).optional().map_err(|e| AppError::internal(e.to_string()))?;
        if let Some(member_id) = member_id {
            if member_id != creator_id && !is_blocked_either_way(conn, creator_id, member_id)? {
                conn.execute("INSERT OR IGNORE INTO chat_members (chat_id, user_id) VALUES (?1, ?2)", params![chat_id, member_id])
                    .map_err(|e| AppError::internal(e.to_string()))?;
            }
        }
    }
    Ok(CreateGroupResponse { chat_id, title: title.to_string() })
}

pub fn is_group_member(conn: &Connection, user_id: i64, chat_id: i64) -> AppResult<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_members WHERE chat_id = ?1 AND user_id = ?2 AND left_at IS NULL)",
        params![chat_id, user_id], |row| row.get(0),
    ).map_err(|e| AppError::internal(e.to_string()))
}

pub fn group_member_ids(conn: &Connection, chat_id: i64) -> AppResult<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT user_id FROM chat_members WHERE chat_id = ?1 AND left_at IS NULL")
        .map_err(|e| AppError::internal(e.to_string()))?;
    let rows = stmt.query_map(params![chat_id], |row| row.get(0)).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(rows.filter_map(Result::ok).collect())
}


pub fn search_messages(conn: &Connection, user_id: i64, q: &str, target_id: Option<i64>, chat_id: Option<i64>) -> AppResult<Vec<MessageSearchResult>> {
    let query = q.trim();
    if query.len() < 2 { return Err(AppError::new(StatusCode::BAD_REQUEST, "Поиск минимум 2 символа")); }
    if query.len() > 128 { return Err(AppError::new(StatusCode::BAD_REQUEST, "Слишком длинный поисковый запрос")); }
    let pattern = format!("%{}%", query);

    if let Some(chat_id) = chat_id {
        if !is_group_member(conn, user_id, chat_id)? { return Err(AppError::new(StatusCode::FORBIDDEN, "Нет доступа к группе")); }
        let title: String = conn.query_row("SELECT title FROM chats WHERE id = ?1 AND deleted_at IS NULL", params![chat_id], |row| row.get(0))
            .map_err(|_| AppError::new(StatusCode::NOT_FOUND, "Группа не найдена"))?;
        let mut stmt = conn.prepare(
            "SELECT m.id, m.sender_id, m.receiver_id, m.chat_id, m.text, m.timestamp, u.login
             FROM messages m JOIN users u ON u.id = m.sender_id
             WHERE m.chat_id = ?1 AND m.text LIKE ?2
             ORDER BY m.id DESC LIMIT 100"
        ).map_err(|e| AppError::internal(e.to_string()))?;
        let rows = stmt.query_map(params![chat_id, pattern], row_to_msg).map_err(|e| AppError::internal(e.to_string()))?;
        return Ok(rows.filter_map(Result::ok).map(|message| MessageSearchResult {
            chat_type: "group".to_string(), chat_id: Some(chat_id), user_id: None, title: title.clone(), message
        }).collect());
    }

    if let Some(target_id) = target_id {
        if is_blocked_either_way(conn, user_id, target_id)? { return Err(AppError::new(StatusCode::FORBIDDEN, "Переписка заблокирована")); }
        let title: String = conn.query_row("SELECT login FROM users WHERE id = ?1 AND deleted_at IS NULL", params![target_id], |row| row.get(0))
            .map_err(|_| AppError::new(StatusCode::NOT_FOUND, "Пользователь не найден"))?;
        let mut stmt = conn.prepare(
            "SELECT m.id, m.sender_id, m.receiver_id, m.chat_id, m.text, m.timestamp, u.login
             FROM messages m JOIN users u ON u.id = m.sender_id
             WHERE m.chat_id IS NULL
               AND ((m.sender_id = ?1 AND m.receiver_id = ?2) OR (m.sender_id = ?2 AND m.receiver_id = ?1))
               AND m.text LIKE ?3
             ORDER BY m.id DESC LIMIT 100"
        ).map_err(|e| AppError::internal(e.to_string()))?;
        let rows = stmt.query_map(params![user_id, target_id, pattern], row_to_msg).map_err(|e| AppError::internal(e.to_string()))?;
        return Ok(rows.filter_map(Result::ok).map(|message| MessageSearchResult {
            chat_type: "direct".to_string(), chat_id: None, user_id: Some(target_id), title: title.clone(), message
        }).collect());
    }

    let mut out = Vec::new();

    let mut direct_stmt = conn.prepare(
        "SELECT m.id, m.sender_id, m.receiver_id, m.chat_id, m.text, m.timestamp, sender.login,
                peer.id, peer.login
         FROM messages m
         JOIN users sender ON sender.id = m.sender_id
         JOIN users peer ON peer.id = CASE WHEN m.sender_id = ?1 THEN m.receiver_id ELSE m.sender_id END
         WHERE m.chat_id IS NULL
           AND (m.sender_id = ?1 OR m.receiver_id = ?1)
           AND m.text LIKE ?2
           AND peer.deleted_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM user_blocks b WHERE b.blocker_id = ?1 AND b.blocked_id = peer.id)
           AND NOT EXISTS (SELECT 1 FROM user_blocks b WHERE b.blocker_id = peer.id AND b.blocked_id = ?1)
         ORDER BY m.id DESC LIMIT 100"
    ).map_err(|e| AppError::internal(e.to_string()))?;
    let direct_rows = direct_stmt.query_map(params![user_id, pattern], |row| {
        Ok((row_to_msg(row)?, row.get::<_, i64>(7)?, row.get::<_, String>(8)?))
    }).map_err(|e| AppError::internal(e.to_string()))?;
    for row in direct_rows.filter_map(Result::ok) {
        out.push(MessageSearchResult { chat_type: "direct".to_string(), chat_id: None, user_id: Some(row.1), title: row.2, message: row.0 });
    }

    let mut group_stmt = conn.prepare(
        "SELECT m.id, m.sender_id, m.receiver_id, m.chat_id, m.text, m.timestamp, sender.login,
                c.id, c.title
         FROM messages m
         JOIN users sender ON sender.id = m.sender_id
         JOIN chats c ON c.id = m.chat_id
         JOIN chat_members cm ON cm.chat_id = c.id AND cm.user_id = ?1 AND cm.left_at IS NULL
         WHERE m.chat_id IS NOT NULL
           AND c.deleted_at IS NULL
           AND m.text LIKE ?2
         ORDER BY m.id DESC LIMIT 100"
    ).map_err(|e| AppError::internal(e.to_string()))?;
    let group_rows = group_stmt.query_map(params![user_id, pattern], |row| {
        Ok((row_to_msg(row)?, row.get::<_, i64>(7)?, row.get::<_, String>(8)?))
    }).map_err(|e| AppError::internal(e.to_string()))?;
    for row in group_rows.filter_map(Result::ok) {
        out.push(MessageSearchResult { chat_type: "group".to_string(), chat_id: Some(row.1), user_id: None, title: row.2, message: row.0 });
    }

    out.sort_by(|a, b| b.message.id.cmp(&a.message.id));
    out.truncate(100);
    Ok(out)
}
