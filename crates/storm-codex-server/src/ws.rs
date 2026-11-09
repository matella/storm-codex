//! WebSocket `/ws` — broadcasts the server events (`match.parsed`, backfill progress) to
//! all the clients (site, OBS overlay, Twitch extension). Source: AppState's broadcast channel.

use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle(socket, state))
}

async fn handle(mut socket: WebSocket, state: AppState) {
    let mut rx = state.events.subscribe();
    loop {
        tokio::select! {
            // server event → client
            ev = rx.recv() => match ev {
                Ok(value) => {
                    let txt = value.to_string();
                    if socket.send(Message::Text(txt.into())).await.is_err() {
                        break; // client parti
                    }
                }
                // lagged behind on the channel: we continue (the events are not critical)
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            // client message (ping/close) — we consume nothing useful, we detect the closing
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
        }
    }
}
