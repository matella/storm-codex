//! Feed live d'une partie en cours vers storm-codex (`/live`).
//!
//! HotS écrit la partie en cours dans `%TEMP%\Heroes of the Storm\TempWriteReplayP1\` :
//! - `replay.server.battlelobby` : complet dès l'écran de chargement → `POST /api/lobby` ;
//! - `replay.tracker.events` / `replay.game.events` : grossissent par blocs de 4 Ko pendant la
//!   partie → on pousse les octets ajoutés à `POST /api/live/chunk` ;
//! - `replay.details` + `replay.initData` apparaissent au flush final → `POST /api/live/end`.
//!
//! Le client ne décode rien : le serveur parse (décision opérateur du 2026-10-24). Contrat serveur :
//! storm-codex `docs/spec/05-api.md`, section « Feed live ».
//!
//! On interroge le dossier périodiquement plutôt que d'utiliser `notify` : le dossier n'existe
//! pas avant la partie, et la taille listée d'un fichier gardé ouvert par le jeu est périmée. La
//! taille se lit donc toujours sur le fichier ouvert (lecture partagée).

use crate::state::SharedState;
use crate::watcher::POLL_INTERVAL;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const LOBBY: &str = "replay.server.battlelobby";
const TRACKER: &str = "replay.tracker.events";
const GAME: &str = "replay.game.events";
const DETAILS: &str = "replay.details";
const INIT_DATA: &str = "replay.initData";

/// Le jeu n'écrit que des blocs pleins pendant la partie : on n'envoie que jusqu'au dernier
/// multiple de 4 Ko, pour ne jamais pousser un bloc à moitié écrit (le serveur passerait la session
/// en `unreadable` pour de bon). La queue partielle part au flush final.
const BLOCK: u64 = 4096;
/// Plafond d'un POST : un rattrapage après une coupure du box reste borné.
const MAX_CHUNK: u64 = 256 * 1024;
/// Après l'apparition de `details` et `initData`, attendre que les deux streams ne bougent plus.
const SETTLE: Duration = Duration::from_secs(2);
/// Nombre maximal de morceaux par stream et par tick lors d’un rattrapage.
const MAX_ROUNDS: usize = 16;

/// Dossier surveillé : `LIVE_DIR` (tests, Docker) sinon `%TEMP%\Heroes of the Storm\TempWriteReplayP1`.
fn default_dir() -> PathBuf {
    std::env::var_os("LIVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join("Heroes of the Storm")
                .join("TempWriteReplayP1")
        })
}

// ---------------------------------------------------------------------------------------------
// Transport

/// Réponse du serveur à un morceau, réduite à ce que le watcher doit en faire.
#[derive(Debug, Clone, PartialEq)]
enum ChunkReply {
    /// `applied` ou `duplicate` : le morceau est acquis.
    Accepted { last_loop: Option<i64> },
    /// `409` : le serveur attend cet offset (redémarrage du box, trou).
    Gap { expected: u64 },
    /// `unreadable` ou `413` : inutile de continuer à pousser cette session.
    Dead(String),
    /// Réseau, `401`, `5xx`… : garder l'offset et réessayer au tick suivant.
    Retry(String),
}

trait Transport {
    /// `Ok(())` si le lobby est acquis (`2xx`, y compris `unchanged`).
    fn lobby(&self, bytes: &[u8]) -> Result<(), String>;
    fn chunk(&self, session: &str, stream: Stream, offset: u64, bytes: &[u8]) -> ChunkReply;
    /// `Ok(())` si la fin est acquise (`2xx`, ou `404` : le serveur ne connaît plus la session).
    fn end(&self, session: &str) -> Result<(), String>;
}

/// Transport HTTP réel : serveur et jeton relus dans l'état partagé (configurables à chaud).
struct HttpTransport {
    state: SharedState,
    agent: ureq::Agent,
}

impl HttpTransport {
    fn new(state: SharedState) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .timeout_global(Some(Duration::from_secs(10)))
                .http_status_as_error(false)
                .build(),
        );
        Self { state, agent }
    }

    fn target(&self, path: &str) -> Result<(String, Option<String>), String> {
        let s = self
            .state
            .lock()
            .map_err(|_| "état verrouillé".to_string())?;
        if s.server_url.is_empty() {
            return Err("aucun serveur configuré".into());
        }
        Ok((
            format!("{}{}", s.server_url.trim_end_matches('/'), path),
            s.auth_token.clone(),
        ))
    }

    fn post(
        &self,
        path: &str,
        content_type: &str,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Result<(u16, String), String> {
        let (url, token) = self.target(path)?;
        let mut req = self.agent.post(&url).header("Content-Type", content_type);
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let mut resp = req.send(body).map_err(|e| format!("réseau : {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        Ok((status, text))
    }
}

impl Transport for HttpTransport {
    fn lobby(&self, bytes: &[u8]) -> Result<(), String> {
        let (status, body) = self.post("/api/lobby", "application/octet-stream", &[], bytes)?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(format!("HTTP {status} {body}"))
        }
    }

    fn chunk(&self, session: &str, stream: Stream, offset: u64, bytes: &[u8]) -> ChunkReply {
        let headers = [
            ("X-Session", session.to_string()),
            ("X-Stream", stream.header().to_string()),
            ("X-Offset", offset.to_string()),
        ];
        match self.post(
            "/api/live/chunk",
            "application/octet-stream",
            &headers,
            bytes,
        ) {
            Ok((status, body)) => parse_chunk_reply(status, &body, offset),
            Err(e) => ChunkReply::Retry(e),
        }
    }

    fn end(&self, session: &str) -> Result<(), String> {
        let body = serde_json::json!({ "session": session }).to_string();
        let (status, text) =
            self.post("/api/live/end", "application/json", &[], body.as_bytes())?;
        if (200..300).contains(&status) || status == 404 {
            Ok(())
        } else {
            Err(format!("HTTP {status} {text}"))
        }
    }
}

/// Traduit une réponse de `POST /api/live/chunk` (contrat `05-api.md`).
fn parse_chunk_reply(status: u16, body: &str, offset: u64) -> ChunkReply {
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let kind = json.get("status").and_then(|v| v.as_str()).unwrap_or("");
    match (status, kind) {
        (200, "applied") | (200, "duplicate") => ChunkReply::Accepted {
            last_loop: json.get("last_loop").and_then(|v| v.as_i64()),
        },
        (200, "unreadable") => ChunkReply::Dead("session illisible côté serveur".into()),
        (409, _) => match json.get("expected_offset").and_then(|v| v.as_u64()) {
            Some(expected) if expected < offset => ChunkReply::Gap { expected },
            _ => ChunkReply::Retry(format!("409 sans expected_offset valide : {body}")),
        },
        (413, _) => ChunkReply::Dead("stream trop gros (413)".into()),
        _ => ChunkReply::Retry(format!("HTTP {status} {body}")),
    }
}

// ---------------------------------------------------------------------------------------------
// Fichiers

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Tracker,
    Game,
}

impl Stream {
    fn file(self) -> &'static str {
        match self {
            Stream::Tracker => TRACKER,
            Stream::Game => GAME,
        }
    }

    fn header(self) -> &'static str {
        match self {
            Stream::Tracker => "tracker",
            Stream::Game => "game",
        }
    }

    fn index(self) -> usize {
        match self {
            Stream::Tracker => 0,
            Stream::Game => 1,
        }
    }
}

/// Ouvre en lecture sans gêner le jeu, qui garde ses fichiers ouverts en écriture.
fn open_shared(path: &Path) -> std::io::Result<File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        opts.share_mode(0x1 | 0x2 | 0x4);
    }
    opts.open(path)
}

/// Taille et date lues **sur le fichier ouvert** (le listing du dossier est périmé).
fn stat(path: &Path) -> Option<(u64, SystemTime)> {
    let meta = open_shared(path).ok()?.metadata().ok()?;
    Some((
        meta.len(),
        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    ))
}

fn stream_len(path: &Path, offset: u64) -> Option<u64> {
    match open_shared(path) {
        Ok(file) => file.metadata().ok().map(|m| m.len()),
        // Un stream déjà lu doit être observable avant de conclure qu'il a rétréci.
        Err(e) if offset == 0 && e.kind() == std::io::ErrorKind::NotFound => Some(0),
        Err(_) => None,
    }
}

fn read_range(path: &Path, from: u64, len: u64) -> std::io::Result<Vec<u8>> {
    let mut f = open_shared(path)?;
    f.seek(SeekFrom::Start(from))?;
    let mut buf = vec![0; len as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// Prochain intervalle à envoyer. Même après un 409 non aligné, on s’arrête sur
/// une frontière de bloc en cours de partie. `MAX_CHUNK` est un multiple de `BLOCK`.
fn plan_chunk(offset: u64, file_len: u64, ended: bool) -> Option<std::ops::Range<u64>> {
    let end = file_len.min(offset.saturating_add(MAX_CHUNK));
    let end = if ended { end } else { end / BLOCK * BLOCK };
    (end > offset).then_some(offset..end)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Machine à états

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Live,
    /// `details` + `initData` vus : on attend que les streams se stabilisent.
    Finishing {
        lens: Option<(u64, u64)>,
        since: Instant,
    },
    /// Fin envoyée, ou partie déjà finie quand on l'a découverte : plus rien à faire.
    Done,
}

#[derive(Debug, Clone, Default)]
struct StreamState {
    offset: u64,
    dead: bool,
}

struct Game {
    session: String,
    lobby: Vec<u8>,
    lobby_mtime: SystemTime,
    lobby_posted: bool,
    phase: Phase,
    streams: [StreamState; 2],
}

/// Ce que le watcher a fait sur un tick — sert aux tests et au journal.
#[derive(Debug, Default, Clone, PartialEq)]
struct TickReport {
    new_session: Option<String>,
    already_finished: bool,
    ended: bool,
}

struct LiveWatcher<T: Transport> {
    dir: PathBuf,
    transport: T,
    game: Option<Game>,
    /// Cache du lobby (taille, date) pour ne pas relire 280 Ko chaque seconde.
    lobby_key: Option<(u64, SystemTime)>,
    settle: Duration,
    log: fn(&str),
}

impl<T: Transport> LiveWatcher<T> {
    fn new(dir: PathBuf, transport: T, log: fn(&str)) -> Self {
        Self {
            dir,
            transport,
            game: None,
            lobby_key: None,
            settle: SETTLE,
            log,
        }
    }

    #[cfg(test)]
    fn with_settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    /// `replay.details` appartient-il à **cette** partie ? Le cycle de vie du dossier entre deux
    /// parties n'est pas mesuré : un `details` résiduel, plus vieux que le lobby, ne doit pas tuer
    /// la partie suivante.
    fn details_of_this_game(&self, lobby_mtime: SystemTime) -> bool {
        match stat(&self.dir.join(DETAILS)) {
            Some((_, mtime)) => mtime >= lobby_mtime,
            None => false,
        }
    }

    fn tick(&mut self, now: Instant) -> TickReport {
        let mut report = TickReport::default();
        let lobby_path = self.dir.join(LOBBY);
        let Some(key @ (lobby_len, lobby_mtime)) = stat(&lobby_path).filter(|(len, _)| *len > 0)
        else {
            return report;
        };

        if self.lobby_key != Some(key) {
            let mut bytes = Vec::new();
            if open_shared(&lobby_path)
                .and_then(|mut f| f.read_to_end(&mut bytes))
                .is_err()
                || bytes.len() as u64 != lobby_len
                || stat(&lobby_path) != Some(key)
            {
                return report;
            }
            self.lobby_key = Some(key);
            let session = sha256_hex(&bytes);
            if self.game.as_ref().map(|g| g.session.as_str()) != Some(session.as_str()) {
                let finished = self.details_of_this_game(lobby_mtime);
                (self.log)(&format!(
                    "live: lobby {} ({} octets){}",
                    &session[..12],
                    lobby_len,
                    if finished {
                        " — partie déjà terminée, aucune session live"
                    } else {
                        ""
                    }
                ));
                report.new_session = Some(session.clone());
                report.already_finished = finished;
                self.game = Some(Game {
                    session,
                    lobby: bytes,
                    lobby_mtime,
                    lobby_posted: false,
                    phase: if finished { Phase::Done } else { Phase::Live },
                    streams: Default::default(),
                });
            }
        }

        let Some(mut game) = self.game.take() else {
            return report;
        };
        self.step(&mut game, now, &mut report);
        self.game = Some(game);
        report
    }

    fn step(&self, game: &mut Game, now: Instant, report: &mut TickReport) {
        if game.phase == Phase::Done {
            return;
        }

        // Le lobby d'abord : sans lui, le serveur ne peut lier ni carte ni opérateur.
        if !game.lobby_posted {
            match self.transport.lobby(&game.lobby) {
                Ok(()) => {
                    game.lobby_posted = true;
                    (self.log)(&format!(
                        "live: session {} démarrée (lobby envoyé)",
                        &game.session[..12]
                    ));
                }
                Err(e) => {
                    (self.log)(&format!("live: lobby non envoyé ({e}) — nouvel essai"));
                    return;
                }
            }
        }

        let tracker_len = stream_len(
            &self.dir.join(TRACKER),
            game.streams[Stream::Tracker.index()].offset,
        );
        let game_len = stream_len(
            &self.dir.join(GAME),
            game.streams[Stream::Game.index()].offset,
        );
        let lens = tracker_len.zip(game_len);
        let ended =
            self.details_of_this_game(game.lobby_mtime) && self.dir.join(INIT_DATA).exists();

        match game.phase {
            Phase::Live if !ended => {
                if let Some(len) = tracker_len {
                    self.push(game, Stream::Tracker, len, false);
                }
                if let Some(len) = game_len {
                    self.push(game, Stream::Game, len, false);
                }
            }
            Phase::Live => {
                (self.log)("live: fin de partie détectée (details + initData)");
                game.phase = Phase::Finishing { lens, since: now };
            }
            Phase::Finishing { lens: prev, since } => {
                if prev != lens {
                    game.phase = Phase::Finishing { lens, since: now };
                    return;
                }
                // Une erreur d’ouverture ne doit pas être prise pour un stream vide.
                let Some((tracker_len, game_len)) = lens else {
                    return;
                };
                if now.duration_since(since) >= self.settle {
                    // Flush final : la queue partielle des deux streams, puis la fin.
                    self.push(game, Stream::Tracker, tracker_len, true);
                    self.push(game, Stream::Game, game_len, true);
                    let caught_up = game
                        .streams
                        .iter()
                        .zip([tracker_len, game_len])
                        .all(|(st, len)| st.dead || st.offset == len);
                    if !caught_up {
                        return; // box injoignable : on réessaie au tick suivant
                    }
                    match self.transport.end(&game.session) {
                        Ok(()) => {
                            (self.log)(&format!(
                                "live: fin envoyée (tracker {} o, game {} o)",
                                tracker_len, game_len
                            ));
                            game.phase = Phase::Done;
                            report.ended = true;
                        }
                        Err(e) => (self.log)(&format!("live: fin non envoyée ({e})")),
                    }
                }
            }
            Phase::Done => {}
        }
    }

    /// Pousse les nouveaux octets du stream, par morceaux de `MAX_CHUNK` au plus.
    fn push(&self, game: &mut Game, stream: Stream, file_len: u64, ended: bool) {
        let path = self.dir.join(stream.file());
        let st = &mut game.streams[stream.index()];
        if st.dead {
            return;
        }
        if file_len < st.offset {
            // Le fichier a rétréci : on repart de zéro, le serveur dédoublonne ce qu'il a déjà.
            (self.log)(&format!(
                "live: {} a rétréci ({} < {}), reprise à 0",
                stream.header(),
                file_len,
                st.offset
            ));
            st.offset = 0;
        }
        for _ in 0..MAX_ROUNDS {
            let Some(range) = plan_chunk(st.offset, file_len, ended) else {
                return;
            };
            let len = range.end - range.start;
            let bytes = match read_range(&path, range.start, len) {
                Ok(bytes) => bytes,
                Err(e) => {
                    (self.log)(&format!("live: lecture {} : {e}", stream.header()));
                    return;
                }
            };
            let sent = bytes.len() as u64;
            match self
                .transport
                .chunk(&game.session, stream, st.offset, &bytes)
            {
                ChunkReply::Accepted { last_loop } => {
                    (self.log)(&format!(
                        "live: {} @{} +{} → loop {}",
                        stream.header(),
                        st.offset,
                        sent,
                        last_loop.map_or("?".into(), |l| l.to_string())
                    ));
                    // Avancer des octets ENVOYÉS, jamais de `consumed` (qui exclut l'événement
                    // coupé en fin de bloc : on renverrait sans fin).
                    st.offset += sent;
                }
                ChunkReply::Gap { expected } => {
                    (self.log)(&format!(
                        "live: {} 409, le serveur attend {} (nous : {})",
                        stream.header(),
                        expected,
                        st.offset
                    ));
                    st.offset = expected;
                    return; // Reprendre au prochain poll, avec les tailles fraîches.
                }
                ChunkReply::Dead(why) => {
                    (self.log)(&format!("live: {} abandonné : {why}", stream.header()));
                    st.dead = true;
                    return;
                }
                ChunkReply::Retry(why) => {
                    (self.log)(&format!(
                        "live: {} @{} en attente : {why}",
                        stream.header(),
                        st.offset
                    ));
                    return;
                }
            }
        }
    }
}

/// Garder le handle en vie tant que le watcher doit tourner ; le drop ferme le canal d’arrêt.
pub struct LiveWatcherHandle {
    _stop_tx: tokio::sync::oneshot::Sender<()>,
}

pub fn start_watcher(state: SharedState, runtime: tokio::runtime::Handle) -> LiveWatcherHandle {
    let dir = default_dir();
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
    println!("Live: watching {}", dir.display());
    runtime.spawn(async move {
        let mut watcher = LiveWatcher::new(dir, HttpTransport::new(state), crate::uploader::log);
        let mut delay = Duration::ZERO;
        loop {
            tokio::select! {
                biased;
                _ = &mut stop_rx => break,
                _ = tokio::time::sleep(delay) => {}
            }
            // Comme l’uploader, ne pas bloquer un worker async avec ureq ou le disque.
            match tokio::task::spawn_blocking(move || {
                watcher.tick(Instant::now());
                watcher
            })
            .await
            {
                Ok(next) => watcher = next,
                Err(e) => {
                    crate::uploader::log(&format!("live: watcher arrêté ({e})"));
                    break;
                }
            }
            delay = POLL_INTERVAL;
        }
    });
    LiveWatcherHandle { _stop_tx: stop_tx }
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{mpsc, Arc, Mutex};

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        Lobby(usize),
        Chunk(&'static str, u64, usize),
        End,
    }

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<Call>>,
        /// Réponses imposées aux prochains morceaux (sinon `Accepted`).
        replies: RefCell<Vec<ChunkReply>>,
        lobby_fails: RefCell<bool>,
    }

    impl Transport for &Fake {
        fn lobby(&self, bytes: &[u8]) -> Result<(), String> {
            self.calls.borrow_mut().push(Call::Lobby(bytes.len()));
            if *self.lobby_fails.borrow() {
                Err("down".into())
            } else {
                Ok(())
            }
        }
        fn chunk(&self, _s: &str, stream: Stream, offset: u64, bytes: &[u8]) -> ChunkReply {
            self.calls
                .borrow_mut()
                .push(Call::Chunk(stream.header(), offset, bytes.len()));
            let mut r = self.replies.borrow_mut();
            if r.is_empty() {
                ChunkReply::Accepted { last_loop: None }
            } else {
                r.remove(0)
            }
        }
        fn end(&self, _s: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(Call::End);
            Ok(())
        }
    }

    impl Fake {
        fn take(&self) -> Vec<Call> {
            std::mem::take(&mut *self.calls.borrow_mut())
        }
    }

    struct Dir(PathBuf);
    impl Dir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("live-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn write(&self, file: &str, bytes: &[u8]) {
            std::fs::write(self.0.join(file), bytes).unwrap();
        }
        fn append(&self, file: &str, n: usize) {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.0.join(file))
                .unwrap();
            f.write_all(&vec![7u8; n]).unwrap();
        }
        fn age(&self, file: &str, secs: u64) {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(self.0.join(file))
                .unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(secs))
                .unwrap();
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn nolog(_: &str) {}

    fn watcher<'a>(d: &Dir, f: &'a Fake) -> LiveWatcher<&'a Fake> {
        LiveWatcher::new(d.0.clone(), f, nolog).with_settle(Duration::ZERO)
    }

    #[test]
    fn partie_deja_finie_au_demarrage_rien_n_est_envoye() {
        let d = Dir::new("finie");
        d.write(LOBBY, b"lobby-A");
        d.append(TRACKER, 9000);
        d.write(DETAILS, b"d");
        d.age(LOBBY, 60);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        let r = w.tick(Instant::now());
        assert!(r.already_finished);
        w.tick(Instant::now());
        assert!(f.take().is_empty());
    }

    #[test]
    fn details_residuel_plus_vieux_que_le_lobby_n_arrete_pas_la_partie() {
        let d = Dir::new("residuel-apres-fin");
        d.write(LOBBY, b"ancien-lobby");
        d.age(LOBBY, 120);
        d.write(DETAILS, b"d");
        d.age(DETAILS, 60);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        assert!(w.tick(Instant::now()).already_finished);
        d.write(LOBBY, b"lobby-B");
        d.append(TRACKER, 4096);
        let r = w.tick(Instant::now());
        assert!(!r.already_finished);
        assert_eq!(
            f.take(),
            vec![Call::Lobby(7), Call::Chunk("tracker", 0, 4096)]
        );
    }

    #[test]
    fn details_residuel_au_demarrage_n_arrete_pas_le_nouveau_lobby() {
        let d = Dir::new("residuel");
        d.write(DETAILS, b"d");
        d.age(DETAILS, 600);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        w.tick(Instant::now()); // Seul le marqueur de la partie précédente est présent.
        assert!(f.take().is_empty());
        d.write(LOBBY, b"lobby-B");
        d.append(TRACKER, 4096);
        let r = w.tick(Instant::now());
        assert!(!r.already_finished);
        assert_eq!(
            f.take(),
            vec![Call::Lobby(7), Call::Chunk("tracker", 0, 4096)]
        );
    }

    #[test]
    fn lobby_d_abord_puis_seulement_les_blocs_pleins() {
        let d = Dir::new("blocs");
        d.write(LOBBY, b"lobby-C");
        d.append(TRACKER, 20480 + 100); // rafale de début + bloc en cours d'écriture
        d.write(GAME, b"");
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        w.tick(Instant::now());
        assert_eq!(
            f.take(),
            vec![Call::Lobby(7), Call::Chunk("tracker", 0, 20480)]
        );

        // Rien de neuf → rien d'envoyé ; le lobby n'est pas renvoyé.
        w.tick(Instant::now());
        assert!(f.take().is_empty());

        // Le bloc se complète, game.events démarre.
        d.append(TRACKER, 4096 - 100);
        d.append(GAME, 4096);
        w.tick(Instant::now());
        assert_eq!(
            f.take(),
            vec![
                Call::Chunk("tracker", 20480, 4096),
                Call::Chunk("game", 0, 4096)
            ]
        );
    }

    #[test]
    fn lobby_refuse_bloque_les_morceaux_jusqu_au_succes() {
        let d = Dir::new("lobbyko");
        d.write(LOBBY, b"lobby-D");
        d.append(TRACKER, 4096);
        let f = Fake::default();
        *f.lobby_fails.borrow_mut() = true;
        let mut w = watcher(&d, &f);
        w.tick(Instant::now());
        assert_eq!(f.take(), vec![Call::Lobby(7)]);
        *f.lobby_fails.borrow_mut() = false;
        w.tick(Instant::now());
        assert_eq!(
            f.take(),
            vec![Call::Lobby(7), Call::Chunk("tracker", 0, 4096)]
        );
    }

    #[test]
    fn conflit_409_renvoie_depuis_l_offset_attendu() {
        let d = Dir::new("gap");
        d.write(LOBBY, b"lobby-E");
        d.append(TRACKER, 8192);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        w.tick(Instant::now());
        f.take();
        // Le box a redémarré : il ne connaît plus rien.
        d.append(TRACKER, 4096);
        f.replies.borrow_mut().push(ChunkReply::Gap { expected: 0 });
        w.tick(Instant::now());
        assert_eq!(f.take(), vec![Call::Chunk("tracker", 8192, 4096)]);
        w.tick(Instant::now());
        assert_eq!(f.take(), vec![Call::Chunk("tracker", 0, 12288)]);
    }

    #[test]
    fn box_injoignable_garde_l_offset_et_rattrape() {
        let d = Dir::new("retry");
        d.write(LOBBY, b"lobby-F");
        d.append(TRACKER, 4096);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        w.tick(Instant::now());
        f.replies
            .borrow_mut()
            .push(ChunkReply::Retry("down".into()));
        d.append(TRACKER, 4096);
        w.tick(Instant::now());
        d.append(TRACKER, 4096);
        w.tick(Instant::now());
        assert_eq!(
            f.take(),
            vec![
                Call::Lobby(7),
                Call::Chunk("tracker", 0, 4096),
                Call::Chunk("tracker", 4096, 4096),
                Call::Chunk("tracker", 4096, 8192)
            ]
        );
    }

    #[test]
    fn session_illisible_arrete_le_stream() {
        let d = Dir::new("dead");
        d.write(LOBBY, b"lobby-G");
        d.append(TRACKER, 4096);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        f.replies.borrow_mut().push(ChunkReply::Dead("x".into()));
        w.tick(Instant::now());
        d.append(TRACKER, 4096);
        w.tick(Instant::now());
        assert_eq!(
            f.take(),
            vec![Call::Lobby(7), Call::Chunk("tracker", 0, 4096)]
        );
    }

    #[test]
    fn rattrapage_decoupe_en_morceaux_bornes() {
        let d = Dir::new("cap");
        d.write(LOBBY, b"lobby-H");
        d.append(TRACKER, (MAX_CHUNK + 8192) as usize);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        w.tick(Instant::now());
        assert_eq!(
            f.take(),
            vec![
                Call::Lobby(7),
                Call::Chunk("tracker", 0, MAX_CHUNK as usize),
                Call::Chunk("tracker", MAX_CHUNK, 8192)
            ]
        );
    }

    #[test]
    fn tracker_recree_sous_le_meme_lobby_repart_a_zero_puis_end() {
        let d = Dir::new("tracker-recree");
        d.write(LOBBY, b"lobby");
        d.age(LOBBY, 60);
        d.append(TRACKER, 4096);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        let t = Instant::now();
        w.tick(t);
        assert_eq!(
            f.take(),
            vec![Call::Lobby(5), Call::Chunk("tracker", 0, 4096)]
        );

        std::fs::remove_file(d.0.join(TRACKER)).unwrap();
        d.write(TRACKER, &[3; 23]);
        d.write(DETAILS, b"d");
        d.write(INIT_DATA, b"i");
        let end_seen = t + POLL_INTERVAL;
        assert!(!w.tick(end_seen).ended);
        assert!(f.take().is_empty());
        assert!(w.tick(end_seen + SETTLE).ended);
        assert_eq!(f.take(), vec![Call::Chunk("tracker", 0, 23), Call::End]);
    }

    #[test]
    fn tracker_temporairement_absent_garde_l_offset_et_flush_avant_end() {
        let d = Dir::new("tracker-absent");
        d.write(LOBBY, b"lobby");
        d.age(LOBBY, 60);
        d.append(TRACKER, 4096);
        let f = Fake::default();
        let mut w = LiveWatcher::new(d.0.clone(), &f, nolog);
        let t = Instant::now();
        w.tick(t);
        assert_eq!(
            f.take(),
            vec![Call::Lobby(5), Call::Chunk("tracker", 0, 4096)]
        );

        d.append(TRACKER, 23);
        d.write(DETAILS, b"d");
        d.write(INIT_DATA, b"i");
        let tracker = d.0.join(TRACKER);
        let moved = d.0.join("tracker-moved");
        std::fs::rename(&tracker, &moved).unwrap();
        let end_seen = t + POLL_INTERVAL;
        for now in [end_seen, end_seen + SETTLE] {
            let report = w.tick(now);
            let offset = w.game.as_ref().unwrap().streams[Stream::Tracker.index()].offset;
            assert_eq!(offset, 4096);
            assert!(!report.ended);
            assert!(f.take().is_empty());
        }

        std::fs::rename(&moved, &tracker).unwrap();
        let restored = end_seen + SETTLE + POLL_INTERVAL;
        assert!(!w.tick(restored).ended);
        assert!(f.take().is_empty());
        assert!(w.tick(restored + SETTLE).ended);
        assert_eq!(f.take(), vec![Call::Chunk("tracker", 4096, 23), Call::End]);
    }

    #[test]
    fn fin_de_partie_flush_la_queue_puis_end() {
        let d = Dir::new("fin");
        d.write(LOBBY, b"lobby-I");
        d.age(LOBBY, 60);
        d.append(TRACKER, 8192);
        d.append(GAME, 4096);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        w.tick(Instant::now());
        f.take();
        // Flush final : queues partielles + details/initData.
        d.append(TRACKER, 1234);
        d.append(GAME, 77);
        d.write(DETAILS, b"d");
        d.write(INIT_DATA, b"i");
        w.tick(Instant::now()); // fin détectée → stabilisation
        assert!(f.take().is_empty());
        let r = w.tick(Instant::now());
        assert!(r.ended);
        assert_eq!(
            f.take(),
            vec![
                Call::Chunk("tracker", 8192, 1234),
                Call::Chunk("game", 4096, 77),
                Call::End
            ]
        );
        w.tick(Instant::now());
        assert!(f.take().is_empty());
    }

    #[test]
    fn nouveau_lobby_nouvelle_session_offsets_remis_a_zero() {
        let d = Dir::new("next");
        d.write(LOBBY, b"lobby-J");
        d.append(TRACKER, 8192);
        d.append(GAME, 8192);
        let f = Fake::default();
        let mut w = watcher(&d, &f);
        let r = w.tick(Instant::now());
        let first = r.new_session.unwrap();
        f.take();
        // Partie suivante : nouveau hash, même taille du lobby.
        d.write(LOBBY, b"lobby-K");
        d.age(LOBBY, 60);
        d.write(TRACKER, &[1u8; 4096]);
        d.write(GAME, &[2u8; 4096]);
        let r = w.tick(Instant::now());
        assert_ne!(r.new_session.unwrap(), first);
        assert_eq!(
            f.take(),
            vec![
                Call::Lobby(7),
                Call::Chunk("tracker", 0, 4096),
                Call::Chunk("game", 0, 4096)
            ]
        );
    }

    #[test]
    fn reponses_du_serveur() {
        assert_eq!(
            parse_chunk_reply(
                200,
                r#"{"status":"applied","consumed":4000,"last_loop":512,"events":9}"#,
                12288
            ),
            ChunkReply::Accepted {
                last_loop: Some(512)
            }
        );
        assert_eq!(
            parse_chunk_reply(200, r#"{"status":"duplicate"}"#, 12288),
            ChunkReply::Accepted { last_loop: None }
        );
        assert_eq!(
            parse_chunk_reply(409, r#"{"status":"gap","expected_offset":8192}"#, 12288),
            ChunkReply::Gap { expected: 8192 }
        );
        assert!(matches!(
            parse_chunk_reply(200, r#"{"status":"unreadable"}"#, 12288),
            ChunkReply::Dead(_)
        ));
        assert!(matches!(
            parse_chunk_reply(413, r#"{"status":"too_large"}"#, 12288),
            ChunkReply::Dead(_)
        ));
        assert!(matches!(
            parse_chunk_reply(401, "", 12288),
            ChunkReply::Retry(_)
        ));
        assert!(matches!(
            parse_chunk_reply(502, "bad gateway", 12288),
            ChunkReply::Retry(_)
        ));
        assert!(matches!(
            parse_chunk_reply(409, r#"{"expected_offset":8192}"#, 4096),
            ChunkReply::Retry(_)
        ));
    }

    #[test]
    fn plan_blocs_pleins_et_croissance() {
        assert_eq!(plan_chunk(0, 4096, false), Some(0..4096));
        assert_eq!(plan_chunk(0, 8193, false), Some(0..8192));
        assert_eq!(plan_chunk(8192, 8193, false), None);
        assert_eq!(plan_chunk(8192, 8192, false), None);
        assert_eq!(plan_chunk(8192, 4096, false), None);
        assert_eq!(plan_chunk(8192, 12288, false), Some(8192..12288));
        assert_eq!(plan_chunk(8192, 8193, true), Some(8192..8193));
    }

    #[test]
    fn rattrapage_non_aligne_se_termine_sur_une_frontiere_de_bloc() {
        let offset = 37;
        let file_len = MAX_CHUNK + BLOCK;
        assert_eq!(plan_chunk(offset, file_len, false), Some(offset..MAX_CHUNK));
        assert_eq!(
            plan_chunk(MAX_CHUNK, file_len, false),
            Some(MAX_CHUNK..file_len)
        );
    }

    #[test]
    fn fin_attend_init_data_la_stabilite_et_le_rattrapage() {
        let d = Dir::new("deferred-end");
        let f = Fake::default();
        let mut w = LiveWatcher::new(d.0.clone(), &f, nolog);
        let t = Instant::now();
        d.write(LOBBY, b"lobby");
        d.age(LOBBY, 60);
        d.append(GAME, 23);
        d.write(TRACKER, b"");
        w.tick(t);
        assert_eq!(f.take(), vec![Call::Lobby(5)]); // Pas de délai de stabilisation du lobby.
        d.write(DETAILS, b"d");
        w.tick(t + POLL_INTERVAL);
        assert!(!w.tick(t + POLL_INTERVAL + SETTLE).ended);
        assert!(f.take().is_empty());
        d.write(INIT_DATA, b"i");
        let end_seen = t + POLL_INTERVAL * 2 + SETTLE;
        w.tick(end_seen);
        d.append(GAME, 10);
        let last_growth = end_seen + POLL_INTERVAL;
        w.tick(last_growth);
        w.tick(end_seen + SETTLE);
        assert!(f.take().is_empty());
        f.replies
            .borrow_mut()
            .push(ChunkReply::Retry("down".into()));
        w.tick(last_growth + SETTLE);
        assert_eq!(f.take(), vec![Call::Chunk("game", 0, 33)]);
        w.tick(last_growth + SETTLE + POLL_INTERVAL);
        assert_eq!(f.take(), vec![Call::Chunk("game", 0, 33), Call::End]);
    }

    struct HttpRequest {
        line: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    impl HttpRequest {
        fn read(stream: &mut TcpStream) -> Self {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut headers = HashMap::new();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" {
                    break;
                }
                let (key, value) = header.split_once(':').unwrap();
                headers.insert(key.to_ascii_lowercase(), value.trim().to_string());
            }
            let mut body = vec![0; headers["content-length"].parse().unwrap()];
            reader.read_exact(&mut body).unwrap();
            Self {
                line,
                headers,
                body,
            }
        }
    }

    #[test]
    fn live_http_loopback_smoke() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let (stop, stopped) = mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = HttpRequest::read(&mut stream);
                        let (status, body) = if requests.len() == 3 {
                            ("409 Conflict", r#"{"status":"gap","expected_offset":0}"#)
                        } else {
                            ("200 OK", r#"{"status":"applied"}"#)
                        };
                        write!(stream,
                            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        ).unwrap();
                        requests.push(request);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if stopped.recv_timeout(Duration::from_millis(1))
                            != Err(mpsc::RecvTimeoutError::Timeout)
                        {
                            return requests;
                        }
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            }
        });

        let state = Arc::new(Mutex::new(crate::state::AppState::new(
            String::new(),
            Some("smoke-token".into()),
            Vec::new(),
            Default::default(),
        )));
        let transport = || HttpTransport {
            state: state.clone(),
            agent: ureq::Agent::new_with_config(
                ureq::config::Config::builder()
                    .proxy(None)
                    .timeout_global(Some(Duration::from_secs(5)))
                    .http_status_as_error(false)
                    .build(),
            ),
        };
        let d = Dir::new("http-smoke");
        let lobby = b"\x00\xfflobby\r\n";
        let session = "a394956ee014c712bec40280feeddfe172ddaba9946966fa46d76817efc506de";
        let block = BLOCK as usize;
        let mut tracker = vec![0x91; block];
        let game = vec![0xe2; block];
        d.write(LOBBY, lobby);
        d.age(LOBBY, 60);
        d.write(TRACKER, &tracker);
        d.write(GAME, &game);
        let mut w = LiveWatcher::new(d.0.clone(), transport(), nolog);
        let t = Instant::now();
        w.tick(t); // Pas d'URL au démarrage ; l'URL est ensuite configurée à chaud.
        state.lock().unwrap().server_url = url;
        w.tick(t + POLL_INTERVAL);
        tracker.extend_from_slice(&vec![0xfe; block]);
        d.write(TRACKER, &tracker);
        w.tick(t + POLL_INTERVAL * 2); // 409 à l'offset 4096.
        w.tick(t + POLL_INTERVAL * 3); // Reprise à 0.
        let tracker_tail = b"\xff\x00tracker";
        let game_tail = b"game\r\n\x00";
        d.write(TRACKER, &[tracker.as_slice(), tracker_tail].concat());
        d.write(GAME, &[game.as_slice(), game_tail].concat());
        d.write(DETAILS, b"d");
        d.write(INIT_DATA, b"i");
        let end_seen = t + POLL_INTERVAL * 4;
        assert!(!w.tick(end_seen).ended);
        assert!(w.tick(end_seen + SETTLE).ended);
        drop(w);

        let mut restarted = LiveWatcher::new(d.0.clone(), transport(), nolog);
        let restart = end_seen + SETTLE + POLL_INTERVAL;
        assert!(restarted.tick(restart).already_finished);
        restarted.tick(restart + SETTLE + POLL_INTERVAL);
        drop(stop);
        let requests = server.join().unwrap();
        assert_eq!(
            requests.len(),
            8,
            "aucun POST supplémentaire au redémarrage"
        );
        for request in &requests {
            assert_eq!(request.headers["authorization"], "Bearer smoke-token");
        }
        assert_eq!(requests[0].line, "POST /api/lobby HTTP/1.1\r\n");
        assert_eq!(
            requests[0].headers["content-type"],
            "application/octet-stream"
        );
        assert_eq!(requests[0].body, lobby);
        for (request, (stream, offset, bytes)) in requests[1..7].iter().zip([
            ("tracker", 0, &tracker[..block]),
            ("game", 0, game.as_slice()),
            ("tracker", block, &tracker[block..]),
            ("tracker", 0, tracker.as_slice()),
            ("tracker", 2 * block, tracker_tail.as_slice()),
            ("game", block, game_tail.as_slice()),
        ]) {
            assert_eq!(request.line, "POST /api/live/chunk HTTP/1.1\r\n");
            assert_eq!(request.headers["content-type"], "application/octet-stream");
            assert_eq!(request.headers["x-session"], session);
            assert_eq!(request.headers["x-stream"], stream);
            assert_eq!(request.headers["x-offset"], offset.to_string());
            assert_eq!(request.body, bytes);
        }
        assert_eq!(requests[7].line, "POST /api/live/end HTTP/1.1\r\n");
        assert_eq!(requests[7].headers["content-type"], "application/json");
        assert_eq!(
            requests[7].body,
            format!(r#"{{"session":"{session}"}}"#).as_bytes()
        );
    }

    #[test]
    fn session_est_le_sha256_hex_du_lobby() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
