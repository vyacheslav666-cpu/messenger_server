use crate::{db, models::IncomingWsMessage, state::AppState};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn handle_user_socket(socket: WebSocket, state: Arc<AppState>, user_id: i64) {
    if user_id <= 0 { return; }

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    {
        let mut conns = state.conns.lock().expect("connections lock poisoned");
        conns.insert(user_id, tx);
    }

    tracing::info!(user_id, "websocket connected");

    let mut send_task = tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if ws_sender.send(Message::Text(text.into())).await.is_err() { break; }
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

            let saved_and_targets = {
                let db = match state_for_recv.db.lock() { Ok(db) => db, Err(_) => break };
                if let Some(chat_id) = incoming.chat_id {
                    match db::save_group_message(&db, user_id, chat_id, &incoming.text) {
                        Ok(saved) => {
                            let targets = db::group_member_ids(&db, chat_id).unwrap_or_default();
                            Ok((saved, targets))
                        }
                        Err(err) => Err(err),
                    }
                } else if let Some(receiver_id) = incoming.receiver_id {
                    match db::save_direct_message(&db, user_id, receiver_id, &incoming.text) {
                        Ok(saved) => Ok((saved, vec![user_id, receiver_id])),
                        Err(err) => Err(err),
                    }
                } else {
                    continue;
                }
            };

            let (saved, targets) = match saved_and_targets {
                Ok(v) => v,
                Err(err) => { tracing::warn!(?err, "message not saved"); continue; }
            };

            let payload = match serde_json::to_string(&saved) { Ok(v) => v, Err(_) => continue };
            let conns_guard = match conns.lock() { Ok(v) => v, Err(_) => break };
            for target_id in targets {
                if let Some(tx) = conns_guard.get(&target_id) {
                    let _ = tx.send(payload.clone());
                }
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
