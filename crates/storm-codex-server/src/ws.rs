//! WebSocket `/ws` — diffuse les événements serveur (`match.parsed`, progression backfill) à
//! tous les clients (site, overlay OBS, extension Twitch). Source : le canal broadcast d'AppState.

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
            // événement serveur → client
            ev = rx.recv() => match ev {
                Ok(value) => {
                    let txt = value.to_string();
                    if socket.send(Message::Text(txt.into())).await.is_err() {
                        break; // client parti
                    }
                }
                // a pris du retard sur le canal : on continue (les events ne sont pas critiques)
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            // message client (ping/close) — on ne consomme rien d'utile, on détecte la fermeture
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
        }
    }
}
