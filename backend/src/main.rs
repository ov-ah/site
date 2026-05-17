use axum::{
    Json, Router,
    extract::{
        ConnectInfo, Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};
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
    uid: Option<i64>,
    color: Option<String>,
    text: String,
    ts: chrono::DateTime<chrono::Utc>,
    is_anon: bool,
}

#[derive(Deserialize)]
struct IncomingMessage {
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

#[derive(Serialize)]
struct MeResponse {
    username: String,
    uid: i64,
    chat_color: String,
    is_admin: bool,
}

// ---- admin types ----

#[derive(Serialize)]
struct AdminUser {
    uid: i64,
    username: String,
    chat_color: String,
    is_admin: bool,
    created_at: String,
    message_count: i64,
    last_ip: Option<String>,
}

#[derive(Serialize)]
struct IpEntry {
    ip: String,
    users: Vec<String>,
    message_count: i64,
    last_seen: Option<String>,
}

#[derive(Serialize)]
struct AdminMessage {
    id: String,
    user: String,
    uid: Option<i64>,
    text: String,
    ts: String,
    is_anon: bool,
    ip: Option<String>,
}

#[derive(Deserialize)]
struct SetColorRequest {
    color: String,
}

#[derive(Deserialize)]
struct SetAdminRequest {
    is_admin: bool,
}

#[derive(Deserialize)]
struct MessagesQuery {
    user: Option<String>,
    ip: Option<String>,
    limit: Option<i64>,
}

// ---- main ----

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
            ts TEXT NOT NULL,
            is_anon INTEGER NOT NULL DEFAULT 0
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

    // backfill migrations — errors are safe to ignore
    for sql in &[
        "ALTER TABLE messages ADD COLUMN is_anon INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE messages ADD COLUMN ip TEXT",
        "ALTER TABLE sessions ADD COLUMN ip TEXT",
        "ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0",
    ] {
        let _ = sqlx::query(sql).execute(&db).await;
    }

    let (tx, _rx) = broadcast::channel(100);
    let state = Arc::new(AppState { tx, db });

    let app = Router::new()
        .route("/", get(|| async { "hello from rust" }))
        .route("/ws", get(ws_handler))
        .route("/history", get(history_handler))
        .route("/register", post(register_handler))
        .route("/login", post(login_handler))
        .route("/logout", post(logout_handler))
        .route("/me", get(me_handler))
        .route("/admin/api/users", get(admin_users_handler))
        .route("/admin/api/users/:uid/color", post(admin_set_color_handler))
        .route("/admin/api/users/:uid/admin", post(admin_set_admin_handler))
        .route("/admin/api/ips", get(admin_ips_handler))
        .route("/admin/api/messages", get(admin_messages_handler))
        .route("/admin/api/messages/:id", delete(admin_delete_message_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on 127.0.0.1:3000");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

// ---- helpers ----

fn extract_ip(headers: &HeaderMap, addr: SocketAddr) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| addr.ip().to_string())
}

fn is_valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|c| c.is_ascii_hexdigit())
}

async fn resolve_user(db: &SqlitePool, jar: &CookieJar) -> Option<(String, i64, String, bool)> {
    let token = jar.get("session")?.value().to_string();

    let row: Option<(String, i64, String, String, bool)> = sqlx::query_as(
        "SELECT u.username, u.uid, u.chat_color, s.expires_at, u.is_admin \
         FROM sessions s JOIN users u ON s.user_id = u.id \
         WHERE s.token = ?",
    )
    .bind(&token)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();

    let (username, uid, chat_color, expires_at, is_admin) = row?;

    if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(&expires_at) {
        if exp.with_timezone(&Utc) < Utc::now() {
            let _ = sqlx::query("DELETE FROM sessions WHERE token = ?")
                .bind(&token)
                .execute(db)
                .await;
            return None;
        }
    }

    Some((username, uid, chat_color, is_admin))
}

async fn admin_guard(db: &SqlitePool, jar: &CookieJar) -> bool {
    resolve_user(db, jar)
        .await
        .map(|(_, _, _, is_admin)| is_admin)
        .unwrap_or(false)
}

// ---- handlers ----

async fn me_handler(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    match resolve_user(&state.db, &jar).await {
        Some((username, uid, chat_color, is_admin)) => Json(MeResponse {
            username,
            uid,
            chat_color,
            is_admin,
        })
        .into_response(),
        None => (StatusCode::UNAUTHORIZED, "not logged in").into_response(),
    }
}

struct HistoryRow {
    id: String,
    user: String,
    text: String,
    ts: String,
    is_anon: i64,
    uid: Option<i64>,
    color: Option<String>,
}

impl sqlx::FromRow<'_, sqlx::sqlite::SqliteRow> for HistoryRow {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> sqlx::Result<Self> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id")?,
            user: row.try_get("user")?,
            text: row.try_get("text")?,
            ts: row.try_get("ts")?,
            is_anon: row.try_get("is_anon")?,
            uid: row.try_get("uid")?,
            color: row.try_get("chat_color")?,
        })
    }
}

async fn history_handler(State(state): State<Arc<AppState>>) -> Json<Vec<ChatMessage>> {
    let rows: Vec<HistoryRow> = sqlx::query_as(
        "SELECT m.id, m.user, m.text, m.ts, m.is_anon, u.uid, u.chat_color \
         FROM messages m LEFT JOIN users u ON m.user = u.username \
         ORDER BY m.ts DESC LIMIT 50",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let messages = rows
        .into_iter()
        .rev()
        .map(|r| {
            let anon = r.is_anon != 0;
            ChatMessage {
                id: r.id,
                user: r.user,
                uid: if anon { None } else { r.uid },
                color: if anon { None } else { r.color },
                text: r.text,
                ts: r.ts.parse().unwrap_or_else(|_| Utc::now()),
                is_anon: anon,
            }
        })
        .collect();

    Json(messages)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user = resolve_user(&state.db, &jar).await;
    let ip = extract_ip(&headers, addr);
    ws.on_upgrade(move |socket| handle_socket(socket, state, user, ip))
}

async fn handle_socket(
    socket: WebSocket,
    state: Arc<AppState>,
    user: Option<(String, i64, String, bool)>,
    ip: String,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    let (display_name, uid_opt, color_opt, is_anon) = match user {
        Some((name, uid, color, _)) => (name, Some(uid), Some(color), false),
        None => {
            let mut bytes = [0u8; 2];
            OsRng.fill_bytes(&mut bytes);
            (format!("anon#{}", hex::encode(bytes)), None, None, true)
        }
    };

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let state_clone = state.clone();
    let display_name_clone = display_name.clone();
    let color_clone = color_opt.clone();
    let ip_clone = ip.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            let Ok(incoming) = serde_json::from_str::<IncomingMessage>(&text) else {
                continue;
            };

            let trimmed = incoming.text.trim();
            if trimmed.is_empty() || trimmed.len() > 500 {
                continue;
            }

            let msg = ChatMessage {
                id: Uuid::new_v4().to_string(),
                user: display_name_clone.clone(),
                uid: uid_opt,
                color: color_clone.clone(),
                text: trimmed.to_string(),
                ts: Utc::now(),
                is_anon,
            };

            if let Err(e) = sqlx::query(
                "INSERT INTO messages (id, user, text, ts, is_anon, ip) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&msg.id)
            .bind(&msg.user)
            .bind(&msg.text)
            .bind(&msg.ts.to_rfc3339())
            .bind(if msg.is_anon { 1 } else { 0 })
            .bind(&ip_clone)
            .execute(&state_clone.db)
            .await
            {
                eprintln!("failed to insert message: {}", e);
            }

            let json = serde_json::to_string(&msg).unwrap();
            let _ = state_clone.tx.send(json);
        }
    });

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
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

async fn create_session(db: &SqlitePool, user_id: &str, ip: &str) -> Result<String, sqlx::Error> {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    let expires_at = Utc::now() + chrono::Duration::days(30);

    sqlx::query(
        "INSERT INTO sessions (token, user_id, expires_at, ip) VALUES (?, ?, ?, ?)",
    )
    .bind(&token)
    .bind(user_id)
    .bind(expires_at.to_rfc3339())
    .bind(ip)
    .execute(db)
    .await?;

    Ok(token)
}

fn validate_credentials(username: &str, password: &str) -> Result<(), &'static str> {
    if username.len() < 2 || username.len() > 32 {
        return Err("username must be 2-32 characters");
    }
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err("username can only contain letters, numbers, _ and -");
    }
    if password.len() < 6 || password.len() > 128 {
        return Err("password must be 6-128 characters");
    }
    Ok(())
}

async fn register_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    if let Err(msg) = validate_credentials(&payload.username, &payload.password) {
        return (jar, (StatusCode::BAD_REQUEST, msg)).into_response();
    }

    let password_hash = match hash_password(&payload.password) {
        Ok(h) => h,
        Err(_) => return (jar, (StatusCode::INTERNAL_SERVER_ERROR, "hash failed")).into_response(),
    };

    let user_id = Uuid::new_v4().to_string();

    let insert_result = sqlx::query(
        "INSERT INTO users (id, username, password_hash, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&payload.username)
    .bind(&password_hash)
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;

    if let Err(e) = insert_result {
        let is_conflict = matches!(&e, sqlx::Error::Database(d) if d.message().contains("UNIQUE"));
        return if is_conflict {
            (jar, (StatusCode::CONFLICT, "username already taken")).into_response()
        } else {
            (jar, (StatusCode::INTERNAL_SERVER_ERROR, "db error")).into_response()
        };
    }

    let ip = extract_ip(&headers, addr);
    let token = match create_session(&state.db, &user_id, &ip).await {
        Ok(t) => t,
        Err(_) => return (jar, (StatusCode::INTERNAL_SERVER_ERROR, "session failed")).into_response(),
    };

    let jar = jar.add(session_cookie(token));
    (jar, StatusCode::OK).into_response()
}

async fn login_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
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
        None => return (jar, (StatusCode::UNAUTHORIZED, "invalid credentials")).into_response(),
    };

    let parsed_hash = match PasswordHash::new(&stored_hash) {
        Ok(h) => h,
        Err(_) => return (jar, (StatusCode::INTERNAL_SERVER_ERROR, "hash parse error")).into_response(),
    };

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return (jar, (StatusCode::UNAUTHORIZED, "invalid credentials")).into_response();
    }

    let ip = extract_ip(&headers, addr);
    let token = match create_session(&state.db, &user_id, &ip).await {
        Ok(t) => t,
        Err(_) => return (jar, (StatusCode::INTERNAL_SERVER_ERROR, "session failed")).into_response(),
    };

    let jar = jar.add(session_cookie(token));
    (jar, StatusCode::OK).into_response()
}

async fn logout_handler(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get("session") {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(cookie.value())
            .execute(&state.db)
            .await;
    }
    let jar = jar.remove(Cookie::from("session"));
    (jar, StatusCode::OK).into_response()
}

// ---- admin handlers ----

async fn admin_users_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !admin_guard(&state.db, &jar).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    let rows: Vec<(i64, String, String, i64, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT u.uid, u.username, u.chat_color, u.is_admin, u.created_at,
                CAST(COUNT(m.id) AS INTEGER) as message_count,
                (SELECT s.ip FROM sessions s
                 WHERE s.user_id = u.id AND s.ip IS NOT NULL
                 ORDER BY s.expires_at DESC LIMIT 1) as last_ip
         FROM users u
         LEFT JOIN messages m ON m.user = u.username AND m.is_anon = 0
         GROUP BY u.uid
         ORDER BY u.uid",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let users: Vec<AdminUser> = rows
        .into_iter()
        .map(|(uid, username, chat_color, is_admin, created_at, message_count, last_ip)| {
            AdminUser {
                uid,
                username,
                chat_color,
                is_admin: is_admin != 0,
                created_at,
                message_count,
                last_ip,
            }
        })
        .collect();

    Json(users).into_response()
}

async fn admin_ips_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !admin_guard(&state.db, &jar).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    // All (ip, username) pairs from sessions
    let session_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT s.ip, u.username
         FROM sessions s JOIN users u ON s.user_id = u.id
         WHERE s.ip IS NOT NULL AND s.ip != ''
         ORDER BY s.ip, u.username",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // Message counts and last seen per IP (all messages including anon)
    let msg_rows: Vec<(String, i64, Option<String>)> = sqlx::query_as(
        "SELECT ip, COUNT(*) as cnt, MAX(ts) as last_seen
         FROM messages WHERE ip IS NOT NULL AND ip != ''
         GROUP BY ip",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut msg_map: std::collections::HashMap<String, (i64, Option<String>)> =
        std::collections::HashMap::new();
    for (ip, cnt, last_seen) in msg_rows {
        msg_map.insert(ip, (cnt, last_seen));
    }

    // Group users by IP
    let mut ip_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (ip, username) in session_rows {
        ip_map.entry(ip).or_default().push(username);
    }

    // Also include IPs that appear only in messages (anon-only IPs)
    for ip in msg_map.keys() {
        ip_map.entry(ip.clone()).or_default();
    }

    let mut entries: Vec<IpEntry> = ip_map
        .into_iter()
        .map(|(ip, users)| {
            let (message_count, last_seen) = msg_map.get(&ip).cloned().unwrap_or((0, None));
            IpEntry { ip, users, message_count, last_seen }
        })
        .collect();

    // Sort: most users first, then most messages
    entries.sort_by(|a, b| {
        b.users.len().cmp(&a.users.len())
            .then_with(|| b.message_count.cmp(&a.message_count))
    });

    Json(entries).into_response()
}

async fn admin_messages_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(params): Query<MessagesQuery>,
) -> impl IntoResponse {
    if !admin_guard(&state.db, &jar).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    let limit = params.limit.unwrap_or(100).min(500);

    let rows: Vec<(String, String, Option<i64>, String, String, i64, Option<String>)> =
        sqlx::query_as(
            "SELECT m.id, m.user, u.uid, m.text, m.ts, m.is_anon, m.ip
             FROM messages m
             LEFT JOIN users u ON m.user = u.username AND m.is_anon = 0
             WHERE (? IS NULL OR m.user = ?)
               AND (? IS NULL OR m.ip = ?)
             ORDER BY m.ts DESC
             LIMIT ?",
        )
        .bind(params.user.as_deref())
        .bind(params.user.as_deref())
        .bind(params.ip.as_deref())
        .bind(params.ip.as_deref())
        .bind(limit)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let messages: Vec<AdminMessage> = rows
        .into_iter()
        .map(|(id, user, uid, text, ts, is_anon, ip)| AdminMessage {
            id,
            user,
            uid,
            text,
            ts,
            is_anon: is_anon != 0,
            ip,
        })
        .collect();

    Json(messages).into_response()
}

async fn admin_set_color_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(uid): Path<i64>,
    Json(payload): Json<SetColorRequest>,
) -> impl IntoResponse {
    if !admin_guard(&state.db, &jar).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !is_valid_color(&payload.color) {
        return (StatusCode::BAD_REQUEST, "invalid color").into_response();
    }

    let result = sqlx::query("UPDATE users SET chat_color = ? WHERE uid = ?")
        .bind(&payload.color)
        .bind(uid)
        .execute(&state.db)
        .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => StatusCode::OK.into_response(),
        Ok(_) => (StatusCode::NOT_FOUND, "user not found").into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn admin_set_admin_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(uid): Path<i64>,
    Json(payload): Json<SetAdminRequest>,
) -> impl IntoResponse {
    if !admin_guard(&state.db, &jar).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    let result = sqlx::query("UPDATE users SET is_admin = ? WHERE uid = ?")
        .bind(if payload.is_admin { 1 } else { 0 })
        .bind(uid)
        .execute(&state.db)
        .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => StatusCode::OK.into_response(),
        Ok(_) => (StatusCode::NOT_FOUND, "user not found").into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn admin_delete_message_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !admin_guard(&state.db, &jar).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    let result = sqlx::query("DELETE FROM messages WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => StatusCode::OK.into_response(),
        Ok(_) => (StatusCode::NOT_FOUND, "message not found").into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
