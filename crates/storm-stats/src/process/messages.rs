//! Messages de partie (chat, pings, BM) et taunts.

use super::*;

/// Messages (parser.js:2214-2247) + détection BM via game events (2249-2346).
/// Décode les game events (~100k/replay) une seule fois.
pub(super) fn process_messages_and_bm(
    data: &Ctx,
    match_: &mut Map<String, J>,
    players: &mut Map<String, J>,
    player_lobby_id: &HashMap<String, String>,
    loop_game_start: f64,
    hotkey_profiles: &mut HashMap<String, String>,
) -> R<()> {
    let loop_to_sec = |loop_: f64| (loop_ - loop_game_start) / 16.0;
    let team_of = |players: &Map<String, J>, handle: &str| -> J {
        players
            .get(handle)
            .and_then(|p| get(p, &["team"]).cloned())
            .unwrap_or(J::Null)
    };

    // --- messages (replay.message.events) ---
    let messages = data.replay.message_events().map_err(Abort::from)?;
    let mtype_loading = rt_int("MessageType", "LoadingProgress");
    let mtype_ping = rt_int("MessageType", "Ping");
    let mtype_chat = rt_int("MessageType", "Chat");
    let mtype_announce = rt_int("MessageType", "PlayerAnnounce");
    let mut msgs: Vec<J> = Vec::new();
    for message in &messages {
        let m = jval(message);
        let mtype = get(&m, &["_eventid"]).and_then(J::as_i64);
        if mtype == Some(mtype_loading) {
            continue;
        }
        // `!(m_userId in playerLobbyID)` → on saute les non-joueurs (observateurs)
        let user_key = js_prop(get(&m, &["_userid", "m_userId"]));
        let Some(player) = player_lobby_id.get(&user_key) else {
            continue;
        };
        let mut msg = Map::new();
        msg.insert(
            "type".into(),
            get(&m, &["_eventid"]).cloned().unwrap_or(J::Null),
        );
        msg.insert("player".into(), J::from(player.as_str()));
        msg.insert("team".into(), team_of(players, player));
        msg.insert(
            "recipient".into(),
            get(&m, &["m_recipient"]).cloned().unwrap_or(J::Null),
        );
        // ordre JS : type, player, team, recipient, loop, time, puis charge utile
        let loop_ = get(&m, &["_gameloop"]).cloned().unwrap_or(J::Null);
        let time = loop_to_sec(js_number(Some(&loop_)));
        msg.insert("loop".into(), loop_);
        msg.insert("time".into(), jnum(time));
        if mtype == Some(mtype_ping) {
            // NB : m_point.x/y suivent le décodeur Blizzard ; hots-parser (port GaryIrick)
            // diffère sur l'interprétation signée (tolérance documentée).
            msg.insert(
                "point".into(),
                json!({
                    "x": get(&m, &["m_point", "x"]).cloned().unwrap_or(J::Null),
                    "y": get(&m, &["m_point", "y"]).cloned().unwrap_or(J::Null),
                }),
            );
        } else if mtype == Some(mtype_chat) {
            msg.insert(
                "text".into(),
                get(&m, &["m_string"]).cloned().unwrap_or(J::Null),
            );
        } else if mtype == Some(mtype_announce) {
            msg.insert(
                "announcement".into(),
                get(&m, &["m_announcement"]).cloned().unwrap_or(J::Null),
            );
        }
        msgs.push(J::Object(msg));
    }
    match_.insert("messages".into(), J::Array(msgs));

    // --- game events : b-step chains + taunts/dances (eventid 27) ---
    let build = match_
        .get("version")
        .and_then(|v| get(v, &["m_build"]))
        .and_then(J::as_f64)
        .unwrap_or(f64::NAN);
    // abilLink de l'action « b » par tranche de build (parser.js:2261-2284)
    let b_abil_link: f64 = if build < 61872.0 {
        200.0
    } else if build < 68740.0 {
        119.0
    } else if build < 70682.0 {
        116.0
    } else if build < 77525.0 {
        112.0
    } else if build < 79033.0 {
        114.0
    } else {
        115.0
    };
    let taunt_abil_link: f64 = if build < 68740.0 { 19.0 } else { 22.0 };
    const BSTEP_FRAME_THRESHOLD: f64 = 8.0;

    // On visite les game events au niveau storm_replay::Value (~100 000/replay) SANS les
    // matérialiser en Vec ni les convertir en JSON : seuls les `SCmdEvent` (eventid 27) avec
    // m_abil pertinent sont traités. `srv_int` lit les champs typés directement.
    fn srv_int(v: &storm_replay::Value, f: &str) -> Option<i64> {
        v.field(f).and_then(storm_replay::Value::as_int)
    }
    // playerBSeq : handle → séquences de (gameloop, m_sequence)
    let mut player_bseq: HashMap<String, Vec<Vec<(f64, f64)>>> = HashMap::new();
    // une « exception JS » dans la visite est capturée ici (la closure ne peut pas `?`)
    let mut abort: Option<Abort> = None;
    data.replay
        .visit_game_events(|ev| {
            let eventid = srv_int(&ev, "_eventid");
            // SUserOptionsEvent (gameloop 0) : profil de raccourcis du joueur, hors parité Node
            // (stocké à part, pas dans les documents joueur).
            if eventid == Some(7) {
                let user_key = ev
                    .field("_userid")
                    .and_then(|u| srv_int(u, "m_userId"))
                    .map(|v| v.to_string());
                let profile = ev.field("m_hotkeyProfile").and_then(|v| v.as_str_lossy());
                if let (Some(k), Some(p)) = (user_key, profile) {
                    if let Some(toon) = player_lobby_id.get(&k) {
                        hotkey_profiles.insert(toon.clone(), p);
                    }
                }
                return;
            }
            if abort.is_some() || eventid != Some(27) {
                return;
            }
            let Some(abil) = ev.field("m_abil") else {
                return;
            };
            if matches!(abil, storm_replay::Value::Null) {
                return;
            }
            // m_abilLink absent → pas de correspondance (comme undefined === N en JS)
            let Some(abil_link) = srv_int(abil, "m_abilLink").map(|v| v as f64) else {
                return;
            };
            let user_id = ev.field("_userid").and_then(|u| srv_int(u, "m_userId"));
            let user_key = user_id.map_or_else(|| "undefined".into(), |v| v.to_string());
            let gameloop = srv_int(&ev, "_gameloop").map_or(f64::NAN, |v| v as f64);
            if abil_link == b_abil_link {
                // chaîne de « b » : id = playerLobbyID[playerID] (undefined → clé "undefined")
                let id = player_lobby_id
                    .get(&user_key)
                    .cloned()
                    .unwrap_or_else(|| "undefined".into());
                let seq = srv_int(&ev, "m_sequence").map_or(f64::NAN, |v| v as f64);
                let seqs = player_bseq.entry(id).or_default();
                if let Some(&(cur_loop, cur_sequence)) = seqs.last().and_then(|s| s.last()) {
                    if (cur_loop - gameloop).abs() <= BSTEP_FRAME_THRESHOLD
                        && (cur_sequence - seq).abs() > 1.0
                    {
                        let last = seqs.len() - 1;
                        seqs[last].push((gameloop, seq));
                    } else {
                        seqs.push(vec![(gameloop, seq)]);
                    }
                } else {
                    seqs.push(vec![(gameloop, seq)]);
                }
            } else if abil_link == taunt_abil_link {
                // taunt (abilCmdIndex 4) ou dance (3) — players[id] undefined → throw (catch JS)
                let Some(id) = player_lobby_id.get(&user_key).cloned() else {
                    abort = Some(Abort::Throw("taunt/dance: joueur non mappé".into()));
                    return;
                };
                let cmd = srv_int(abil, "m_abilCmdIndex").map_or(f64::NAN, |v| v as f64);
                let field = if cmd == 4.0 {
                    Some("taunts")
                } else if cmd == 3.0 {
                    Some("dances")
                } else {
                    None
                };
                if let Some(field) = field {
                    let obj = json!({
                        "loop": jnum(gameloop), "time": jnum(loop_to_sec(gameloop)),
                        "kills": 0, "deaths": 0,
                    });
                    match players
                        .get_mut(&id)
                        .and_then(J::as_object_mut)
                        .and_then(|p| p.get_mut(field))
                        .and_then(J::as_array_mut)
                    {
                        Some(a) => a.push(obj),
                        None => {
                            abort = Some(Abort::Throw(format!("taunt/dance: joueur {id} inconnu")))
                        }
                    }
                }
            }
        })
        .map_err(Abort::from)?;
    if let Some(a) = abort {
        return Err(a);
    }
    process_taunt_data(match_, players, &player_bseq)?;
    Ok(())
}

/// Contexte kills/deaths des bsteps/taunts/voiceLines/sprays/dances (parser.js:2460-2592).
/// Reproduit fidèlement le bug : la boucle `dances` est imbriquée dans la boucle `sprays`,
/// donc les dances ne reçoivent leur contexte que si le joueur a ≥ 1 spray, et l'accumulation
/// est répétée `sprays.length` fois.
pub(super) fn process_taunt_data(
    match_: &Map<String, J>,
    players: &mut Map<String, J>,
    player_bseq: &HashMap<String, Vec<Vec<(f64, f64)>>>,
) -> R<()> {
    // takedowns pré-extraits : (loop, victim.player, [killer.player])
    let takedowns: Vec<(f64, J, Vec<J>)> = match_
        .get("takedowns")
        .and_then(J::as_array)
        .map(|arr| {
            arr.iter()
                .map(|td| {
                    let loop_ = js_number(get(td, &["loop"]));
                    let victim = get(td, &["victim", "player"]).cloned().unwrap_or(J::Null);
                    let killers = get(td, &["killers"])
                        .and_then(J::as_array)
                        .map(|ks| {
                            ks.iter()
                                .map(|k| get(k, &["player"]).cloned().unwrap_or(J::Null))
                                .collect()
                        })
                        .unwrap_or_default();
                    (loop_, victim, killers)
                })
                .collect()
        })
        .unwrap_or_default();

    // compte (deaths, kills) dans une fenêtre [min, max] pour un joueur donné
    let context = |min: f64, max: f64, id: &str| -> (i64, i64) {
        let idj = J::from(id);
        let (mut kills, mut deaths) = (0i64, 0i64);
        for (time, victim, killers) in &takedowns {
            if min <= *time && *time <= max {
                if js_strict_eq(Some(victim), Some(&idj)) {
                    deaths += 1;
                }
                if killers.iter().any(|k| js_strict_eq(Some(k), Some(&idj))) {
                    kills += 1;
                }
            }
        }
        (kills, deaths)
    };

    // bsteps (ordre des ids sans incidence : chaque id écrit dans un joueur distinct)
    for (id, seqs) in player_bseq {
        for seq in seqs {
            if seq.len() > 2 {
                let start = seq[0].0;
                let stop = seq[seq.len() - 1].0;
                let (kills, deaths) = context(start - 80.0, stop + 80.0, id);
                let bstep = json!({
                    "start": jnum(start), "stop": jnum(stop), "duration": jnum(stop - start),
                    "kills": kills, "deaths": deaths,
                });
                players
                    .get_mut(id)
                    .and_then(J::as_object_mut)
                    .and_then(|p| p.get_mut("bsteps"))
                    .and_then(J::as_array_mut)
                    .ok_or_else(|| Abort::Throw(format!("bsteps: joueur {id} inconnu")))?
                    .push(bstep);
            }
        }
    }

    // taunts / voiceLines / sprays(+dances imbriqué) — ordre d'insertion des joueurs
    let order: Vec<String> = players.keys().cloned().collect();
    for id in &order {
        // collecte des loops par catégorie (emprunt immuable d'abord, écriture ensuite)
        let (taunt_loops, voice_loops, spray_loops, dance_loops) = {
            let p = players.get(id).and_then(J::as_object);
            let loops = |p: Option<&Map<String, J>>, field: &str| -> Vec<f64> {
                p.and_then(|p| p.get(field))
                    .and_then(J::as_array)
                    .map(|a| a.iter().map(|e| js_number(get(e, &["loop"]))).collect())
                    .unwrap_or_default()
            };
            (
                loops(p, "taunts"),
                loops(p, "voiceLines"),
                loops(p, "sprays"),
                loops(p, "dances"),
            )
        };

        let apply = |players: &mut Map<String, J>, field: &str, i: usize, k: i64, d: i64| {
            if let Some(ev) = players
                .get_mut(id)
                .and_then(J::as_object_mut)
                .and_then(|p| p.get_mut(field))
                .and_then(J::as_array_mut)
                .and_then(|a| a.get_mut(i))
                .and_then(J::as_object_mut)
            {
                if let Some(kn) = ev.get("kills").and_then(J::as_i64) {
                    ev.insert("kills".into(), J::from(kn + k));
                }
                if let Some(dn) = ev.get("deaths").and_then(J::as_i64) {
                    ev.insert("deaths".into(), J::from(dn + d));
                }
            }
        };

        for (i, &t) in taunt_loops.iter().enumerate() {
            let (k, d) = context(t - 80.0, t + 80.0, id); // |x|<=80 ⇔ fenêtre symétrique
                                                          // parser.js utilise Math.abs(tauntTime - time) <= 80 (fenêtre identique)
            apply(players, "taunts", i, k, d);
        }
        for (i, &t) in voice_loops.iter().enumerate() {
            let (k, d) = context(t - 80.0, t + 80.0, id);
            apply(players, "voiceLines", i, k, d);
        }
        for (i, &s) in spray_loops.iter().enumerate() {
            let (k, d) = context(s - 80.0, s + 80.0, id);
            apply(players, "sprays", i, k, d);
            // BUG reproduit : boucle dances imbriquée → s'exécute pour CHAQUE spray
            for (j, &dl) in dance_loops.iter().enumerate() {
                let (k, d) = context(dl - 80.0, dl + 80.0, id);
                apply(players, "dances", j, k, d);
            }
        }
    }
    Ok(())
}
