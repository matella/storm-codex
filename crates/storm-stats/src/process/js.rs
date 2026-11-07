//! Primitives de sémantique JavaScript (dates, nombres, `for…in`, accès JSON) : le port reproduit
//! parser.js à l'identique, y compris ses conversions implicites.

use super::*;

/// `new Date(ms)` JS sérialisé par JSON.stringify : ISO 8601 UTC avec millisecondes.
pub(super) fn js_date_iso(ms: f64) -> String {
    let total_ms = ms.trunc() as i64;
    let (mut days, mut rem) = (
        total_ms.div_euclid(86_400_000),
        total_ms.rem_euclid(86_400_000),
    );
    let _ = &mut days;
    let msec = rem % 1000;
    rem /= 1000;
    let (h, m, s) = (rem / 3600, (rem / 60) % 60, rem % 60);
    // civil_from_days (Howard Hinnant) — époque 1970-01-01
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.{msec:03}Z")
}

pub(super) fn win_file_time_to_date(filetime: f64) -> String {
    js_date_iso(filetime / 10_000.0 - 11_644_473_600_000.0)
}

/// `Number` JS sérialisé : NaN/Infinity → null (JSON.stringify).
pub(super) fn jnum(f: f64) -> J {
    serde_json::Number::from_f64(f).map_or(J::Null, J::Number)
}

/// `Math.max(a, b)` JS : NaN si un des arguments est NaN (≠ `f64::max` Rust).
pub(super) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `Math.min(a, b)` JS : NaN si un des arguments est NaN (≠ `f64::min` Rust).
pub(super) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// Coercition `Number(x)` JS sur nos valeurs JSON : undefined → NaN, null → 0, bool → 0/1,
/// chaîne → parse décimal (vide → 0). Arrays/objets : jamais atteints sur nos données.
pub(super) fn js_number(v: Option<&J>) -> f64 {
    match v {
        None => f64::NAN,
        Some(J::Null) => 0.0,
        Some(J::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(J::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(J::String(s)) => {
            let t = s.trim_matches(is_js_ws);
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Some(_) => f64::NAN,
    }
}

/// Truthiness JS (`if (x)`, `!x`) sur nos valeurs JSON.
pub(super) fn js_truthy(v: Option<&J>) -> bool {
    match v {
        None | Some(J::Null) => false,
        Some(J::Bool(b)) => *b,
        Some(J::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(J::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `ToString(number)` JS — suffisant pour nos grandeurs (jamais de notation exponentielle) :
/// le Display Rust est aussi la plus courte représentation qui re-parse à l'identique.
pub(super) fn js_num_str(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if x == 0.0 {
        "0".into() // couvre -0 (JS : String(-0) === "0")
    } else {
        format!("{x}")
    }
}

/// `parseInt(<nombre>)` JS (ToString puis parseInt) : pour nos grandeurs (< 1e21, jamais de
/// notation exponentielle), équivaut à une troncature vers zéro ; NaN/Infinity → NaN.
pub(super) fn js_parse_int_num(x: f64) -> f64 {
    if x.is_finite() {
        x.trunc()
    } else {
        f64::NAN
    }
}

/// `loopsToSeconds` (parser.js:3335-3338) : 16 boucles par seconde.
pub(super) fn loops_to_seconds(loops: f64) -> f64 {
    loops / 16.0
}

/// Ordre d'itération `for..in` JS : indices de tableau canoniques en ordre croissant,
/// puis les autres clés en ordre d'insertion.
pub(super) fn js_for_in_keys(o: &Map<String, J>) -> Vec<String> {
    let mut idx: Vec<(u32, String)> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    for k in o.keys() {
        match k.parse::<u32>() {
            Ok(n) if n.to_string() == *k && n != u32::MAX => idx.push((n, k.clone())),
            _ => rest.push(k.clone()),
        }
    }
    idx.sort_by_key(|(n, _)| *n);
    idx.into_iter().map(|(_, k)| k).chain(rest).collect()
}

/// Comparateur de tri JS « a < b → -1, a > b → 1, sinon 0 » (NaN → Equal/Greater incohérent
/// comme en JS ; jamais rencontré sur nos données). `sort_by` Rust est stable comme TimSort V8.
pub(super) fn js_cmp(a: f64, b: f64) -> std::cmp::Ordering {
    if a < b {
        std::cmp::Ordering::Less
    } else if a > b {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// `combineIntervals` (parser.js:3084-3114) : tri par borne gauche puis fusion par
/// chevauchement (`prev[1] >= c[0]`, faux si NaN — comme en JS).
pub(super) fn combine_intervals(mut intervals: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if intervals.len() <= 1 {
        return intervals;
    }
    intervals.sort_by(|a, b| js_cmp(a.0, b.0));
    let mut result = Vec::new();
    let mut prev = intervals[0];
    for c in &intervals[1..] {
        if prev.1 >= c.0 {
            prev = (prev.0, js_max(prev.1, c.1));
        } else {
            result.push(prev);
            prev = *c;
        }
    }
    result.push(prev);
    result
}

/// Classe `\s` des regex JS (≠ `char::is_whitespace` Rust : pas U+0085, mais U+FEFF).
pub(super) fn is_js_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `key.replace(/\s+/g, '')` de parser.js:826.
pub(super) fn js_strip_ws(s: &str) -> String {
    s.chars().filter(|c| !is_js_ws(*c)).collect()
}

/// Clé de propriété JS (`obj[v]`) pour nos cas : nombre → `ToString(number)` JS (un Number
/// f64 entier comme `jnum(1.0)` donne "1", pas "1.0"), null → "null", undefined → "undefined".
pub(super) fn js_prop(v: Option<&J>) -> String {
    match v {
        None => "undefined".into(),
        Some(J::Null) => "null".into(),
        Some(J::Bool(b)) => b.to_string(),
        Some(J::Number(n)) => js_num_str(n.as_f64().unwrap_or(f64::NAN)),
        Some(J::String(s)) => s.clone(),
        // jamais atteint sur nos données (arrays/objets : ToString JS différerait)
        Some(other) => other.to_string(),
    }
}

/// `parseInt` JS : préfixe numérique (signe + chiffres), NaN → None.
pub(super) fn js_parse_int(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().ok().map(|v| sign * v)
}

/// Accès générique « à la JS » : index manquant → undefined (None).
pub(super) fn get<'a>(v: &'a J, path: &[&str]) -> Option<&'a J> {
    let mut cur = v;
    for p in path {
        cur = match cur {
            J::Object(o) => o.get(*p)?,
            J::Array(a) => a.get(p.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

pub(super) fn get_str<'a>(v: &'a J, path: &[&str]) -> Option<&'a str> {
    get(v, path).and_then(J::as_str)
}
