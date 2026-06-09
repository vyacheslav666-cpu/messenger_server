use crate::{db, models::IncomingWsMessage, state::AppState};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn handle_user_socket(socket: WebSocket, state: Arc<AppState>, user_id: i64) {
    if user_id <= 0 {
        return;
    }

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    {
        let mut conns = state.conns.lock().expect("connections lock poisoned");
        conns.insert(user_id, tx);
    }

    tracing::info!(user_id, "websocket connected");

    let mut send_task = tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if ws_sender.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    let conns = state.conns.clone();
    let state_for_recv = state.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = ws_receiver.next().await {
            let incoming = match serde_json::from_str::<IncomingWsMessage>(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let saved = {
                let db = match state_for_recv.db.lock() {
                    Ok(db) => db,
                    Err(_) => break,
                };
                db::save_message(&db, user_id, incoming.receiver_id, &incoming.text)
            };

            let saved = match saved {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!(?err, "message not saved");
                    continue;
                }
            };

            let payload = match serde_json::to_string(&saved) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let conns_guard = match conns.lock() {
                Ok(v) => v,
                Err(_) => break,
            };

            if let Some(receiver_tx) = conns_guard.get(&saved.receiver_id) {
                let _ = receiver_tx.send(payload.clone());
            }
            if let Some(sender_tx) = conns_guard.get(&saved.sender_id) {
                let _ = sender_tx.send(payload);
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }

    let mut conns = state.conns.lock().expect("connections lock poisoned");
    conns.remove(&user_id);
    tracing::info!(user_id, "websocket disconnected");
}
