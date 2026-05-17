use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

use argon2::password_hash::{
    SaltString,
    rand_core::{OsRng, RngCore},
};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::Utc;
use uuid::Uuid;

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

#[derive(Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[tokio::main]
async fn main() {
    let db = SqlitePool::connect("sqlite://chat.db?mode=rwc")
        .await
        .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            user TEXT NOT NULL,
            text TEXT NOT NULL,
            ts TEXT NOT NULL
        )",
    )
    .execute(&db)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
        uid INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT UNIQUE NOT NULL,
        username TEXT UNIQUE NOT NULL,
        password_hash TEXT NOT NULL,
        chat_color TEXT NOT NULL DEFAULT '#888888',
        created_at TEXT NOT NULL
    )",
    )
    .execute(&db)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
        token TEXT PRIMARY KEY,
        user_id TEXT NOT NULL REFERENCES users(id),
        expires_at TEXT NOT NULL
    )",
    )
    .execute(&db)
    .await
    .unwrap();

    let (tx, _rx) = broadcast::channel::<String>(100);
    let state = Arc::new(AppState { tx, db });

    let app = Router::new()
        .route("/", get(|| async { "hello from rust" }))
        .route("/ws", get(ws_handler))
        .route("/history", get(history_handler))
        .route("/register", post(register_handler))
        .route("/login", post(login_handler))
        .route("/logout", post(logout_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on 127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}

async fn history_handler(State(state): State<Arc<AppState>>) -> Json<Vec<ChatMessage>> {
    let rows: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT id, user, text, ts FROM messages ORDER BY ts DESC LIMIT 50")
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

    let messages: Vec<ChatMessage> = rows
        .into_iter()
        .rev()
        .map(|(id, user, text, ts)| ChatMessage {
            id,
            user,
            text,
            ts: ts.parse().unwrap_or_else(|_| chrono::Utc::now()),
        })
        .collect();

    Json(messages)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // forward broadcast message out to this client
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let state_clone = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
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
                .bind(&msg.id)
                .bind(&msg.user)
                .bind(&msg.text)
                .bind(&msg.ts.to_rfc3339())
                .execute(&state_clone.db)
                .await;
            let json = serde_json::to_string(&msg).unwrap();
            println!("received from {}: {}", msg.user, msg.text);
            let _ = state_clone.tx.send(json);
        }
    });

    // if either task ends, abort
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

fn session_cookie(token: String) -> Cookie<'static> {
    Cookie::build(("session", token))
        .http_only(true)
        .secure(false)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::days(30))
        .build()
}

fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

async fn create_session(db: &SqlitePool, user_id: &str) -> Result<String, sqlx::Error> {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);

    let token = hex::encode(bytes);
    let expires_at = Utc::now() + chrono::Duration::days(30);

    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES (?, ?, ?)")
        .bind(&token)
        .bind(user_id)
        .bind(expires_at.to_rfc3339())
        .execute(db)
        .await?;

    Ok(token)
}

async fn register_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    let password_hash = match hash_password(&payload.password) {
        Ok(h) => h,
        Err(_) => {
            return (
                jar,
                (StatusCode::INTERNAL_SERVER_ERROR, "hash failed").into_response(),
            )
                .into_response();
        }
    };

    let user_id = Uuid::new_v4().to_string();

    let insert_result = sqlx::query(
        "INSERT INTO users (id, username, password_hash, created_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&payload.username)
    .bind(&password_hash)
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;

    if insert_result.is_err() {
        return (
            jar,
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
        )
            .into_response();
    }

    let token = match create_session(&state.db, &user_id).await {
        Ok(t) => t,
        Err(_) => {
            return (
                jar,
                (StatusCode::INTERNAL_SERVER_ERROR, "session failed").into_response(),
            )
                .into_response();
        }
    };

    let jar = jar.add(session_cookie(token));
    (jar, (StatusCode::OK, "registered").into_response()).into_response()
}

async fn login_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE username = ?")
            .bind(&payload.username)
            .fetch_optional(&state.db)
            .await
            .unwrap_or(None);

    let (user_id, stored_hash) = match row {
        Some(r) => r,
        None => {
            return (
                jar,
                (StatusCode::UNAUTHORIZED, "invalid credentials").into_response(),
            )
                .into_response();
        }
    };

    let parsed_hash = match PasswordHash::new(&stored_hash) {
        Ok(h) => h,
        Err(_) => {
            return (
                jar,
                (StatusCode::INTERNAL_SERVER_ERROR, "hash parse error").into_response(),
            )
                .into_response();
        }
    };

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return (
            jar,
            (StatusCode::UNAUTHORIZED, "invalid credentials").into_response(),
        )
            .into_response();
    }

    let token = match create_session(&state.db, &user_id).await {
        Ok(t) => t,
        Err(_) => {
            return (
                jar,
                (StatusCode::INTERNAL_SERVER_ERROR, "session failed").into_response(),
            )
                .into_response();
        }
    };

    let jar = jar.add(session_cookie(token));
    (jar, (StatusCode::OK, "logged in").into_response()).into_response()
}

async fn logout_handler(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get("session") {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(cookie.value())
            .execute(&state.db)
            .await;
    }

    let jar = jar.remove(Cookie::from("session"));
    (jar, (StatusCode::OK, "logged out").into_response()).into_response()
}
