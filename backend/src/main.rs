use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<String>,
    db: SqlitePool,
}

#[derive(Serialize, Deserialize, Clone)]
struct ChatMessage {
    id: String,
    user: String,
    text: String,
    ts: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct IncomingMessage {
    user: String,
    text: String,
}

#[tokio::main]
async fn main() {
    let db = SqlitePool::connect("sqlite://chat.db?mode=rwc").await.unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            user TEXT NOT NULL,
            text TEXT NOT NULL,
            ts TEXT NOT NULL
        )"
    ).execute(&db).await.unwrap();

    let (tx, _rx) = broadcast::channel::<String>(100);
    let state = Arc::new(AppState { tx, db });

    let app = Router::new()
        .route("/", get(|| async { "hello from rust" }))
        .route("/ws", get(ws_handler))
        .route("/history", get(history_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("listening on 127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}

async fn history_handler(State(state): State<Arc<AppState>>) -> Json<Vec<ChatMessage>> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, user, text, ts FROM messages ORDER BY ts DESC LIMIT 50"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let messages: Vec<ChatMessage> = rows.into_iter().rev().map(|(id, user, text, ts)| {
        ChatMessage {
            id, user, text,
            ts: ts.parse().unwrap_or_else(|_| chrono::Utc::now()),
        }
    }).collect();

    Json(messages)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,) -> impl IntoResponse {
    ws.on_upgrade(|socket|handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut reciever) = socket.split();
    let mut rx = state.tx.subscribe();

    // forward broadcast message out to this client
    let mut send_task = tokio::spawn(async move{
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let state_clone = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = reciever.next().await {
            let Ok(incoming) = serde_json::from_str::<IncomingMessage>(&text) else {
                println!("bad message format: {}", text);
                continue;
            };
            let msg = ChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                user: incoming.user,
                text: incoming.text,
                ts: chrono::Utc::now(),
            };
            let _ = sqlx::query("INSERT INTO messages (id, user, text, ts) VALUES (?, ?, ?, ?)")
                .bind(&msg.id).bind(&msg.user).bind(&msg.text).bind(&msg.ts.to_rfc3339())
                .execute(&state_clone.db).await;
            let json = serde_json::to_string(&msg).unwrap();
            println!("recieved from {}: {}", msg.user, msg.text);
            let _ = state_clone.tx.send(json);
        }
    });
    
    // if either task ends, abort
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}
