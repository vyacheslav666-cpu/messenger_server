use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
    routing::get,
    Router,
    http::Method,
};
use tower_http::cors::{Any, CorsLayer};
use std::net::SocketAddr;
use tokio::sync::broadcast;
// Вот эти две строчки заставят .split() и .next() компилироваться:
use futures_util::{SinkExt, StreamExt};

// Создаем структуру для хранения канала рассылки
#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() {
    // Создаем канал рассылки: буфер на 16 сообщений
    let (tx, _rx) = broadcast::channel(16);
    let state = AppState { tx };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(vec![Method::GET, Method::POST]);

    // Передаем state в роутер
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Сервер многопользовательского чата запущен на http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    // Делим сокет на отправку (sender) и чтение (receiver)
    let (mut sender, mut receiver) = socket.split();

    // Подписываемся на общий канал сообщений
    let mut rx = state.tx.subscribe();

    // Задача 1: Рассылать сообщения из общего канала этому клиенту
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break; 
            }
        }
    });

    // Задача 2: Слушать этого клиента и кидать его сообщения в общий канал
    let tx = state.tx.clone();
    let mut receiver_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            let _ = tx.send(text.to_string());
        }
    });

    // Убиваем обе задачи, если клиент отключился
    tokio::select! {
        _ = (&mut send_task) => receiver_task.abort(),
        _ = (&mut receiver_task) => send_task.abort(),
    };
}