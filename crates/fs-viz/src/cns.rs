//! CNS / Luna Light illustration for Inspect in StarDust.
//!
//! BoundClass and the WorldLay region are not `Send`. HTTP is multi-threaded, so
//! Python calls and region access run on one worker thread. Open returns after
//! `import_all` + catalog + one hash/spiral/`spread_layout` of every word (import
//! overlay covers that). Same path reuses the worker Session (no second
//! `import_all`; `placed` already filled → skip layout). `layout_seed` is
//! process-lifetime; mixed into `leaf_axis` and spiral phase only. Default plane
//! is that cloud, not Jaccard embedding and not a rim-growing disk. Neighborhood
//! is Jaccard (cap 80) display-only. `persist_encodings` runs after the first
//! Stars reply.

#[cfg(not(feature = "python"))]
use serde_json::json;
#[cfg(not(feature = "python"))]
use crate::host::{ApiError, ApiResult};

#[cfg(not(feature = "python"))]
const NO_PY: &str = "fs-viz built without python (cargo run -p fs-viz --features python)";

#[cfg(not(feature = "python"))]
pub fn status() -> ApiResult {
    Ok(json!({ "ok": false, "python": false, "reason": NO_PY }))
}

#[cfg(not(feature = "python"))]
pub fn probe(_path: &str) -> ApiResult {
    Ok(json!({
        "ok": true,
        "canInspect": false,
        "python": false,
        "reason": NO_PY,
    }))
}

#[cfg(not(feature = "python"))]
pub fn open(_path: &str) -> ApiResult {
    Err(ApiError::bad(NO_PY))
}

#[cfg(not(feature = "python"))]
pub fn stars(
    _session: u64,
    _xmin: f64,
    _xmax: f64,
    _ymin: f64,
    _ymax: f64,
    _scale: f64,
    _focus: Option<&str>,
    _jump: Option<&str>,
) -> ApiResult {
    Err(ApiError::bad(NO_PY))
}

#[cfg(not(feature = "python"))]
pub fn search(_session: u64, _q: &str) -> ApiResult {
    Err(ApiError::bad(NO_PY))
}

#[cfg(not(feature = "python"))]
pub fn query(_session: u64, _ch: &str) -> ApiResult {
    Err(ApiError::bad(NO_PY))
}

#[cfg(feature = "python")]
pub use live::{open, probe, query, search, stars, status};

#[cfg(feature = "python")]
mod live {
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{self, RecvTimeoutError, Sender};
    use std::sync::OnceLock;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde_json::{json, Value as Json};
    use stardust::{BoundClass, Ir, MethodLoader, Path as IrPath, Region, Type, Value};

    use crate::host::{ApiError, ApiResult};

    const OPEN_TIMEOUT: Duration = Duration::from_secs(180);
    const CALL_TIMEOUT: Duration = Duration::from_secs(45);
    const STAR_CAP: usize = 700;
    const NEIGHBOR_CAP: usize = 80;
    const MIN_SHARED_LEAVES: u32 = 1;
    const PLANE_SCALE: f64 = 1600.0;
    const GOLDEN: f64 = 2.399_963_229_728_653;
    const IDC: &[char] = &[
        '⿰', '⿱', '⿲', '⿳', '⿴', '⿵', '⿶', '⿷', '⿸', '⿹', '⿺', '⿻',
    ];

    static REQ: OnceLock<Sender<Req>> = OnceLock::new();
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    /// Process-lifetime layout seed. 0 = unset; first session stores mixed
    /// `SystemTime` nanos. Reused for every Inspect until the process quits.
    static LAYOUT_SEED: AtomicU64 = AtomicU64::new(0);

    enum Req {
        Status(Sender<ApiResult>),
        Probe {
            path: String,
            tx: Sender<ApiResult>,
        },
        Open {
            path: String,
            tx: Sender<ApiResult>,
        },
        Stars {
            session: u64,
            xmin: f64,
            xmax: f64,
            ymin: f64,
            ymax: f64,
            scale: f64,
            focus: Option<String>,
            jump: Option<String>,
            tx: Sender<ApiResult>,
        },
        Search {
            session: u64,
            q: String,
            tx: Sender<ApiResult>,
        },
        Query {
            session: u64,
            ch: String,
            tx: Sender<ApiResult>,
        },
    }

    struct WordStar {
        ch: String,
        cns: String,
        ids: String,
        seq_level: u8,
        has_seq_level: u8,
        encoding: String,
        post: String,
        leaves: Vec<String>,
        leaf_set: HashSet<u64>,
        x: f64,
        y: f64,
        z: f64,
    }

    struct Session {
        id: u64,
        path: PathBuf,
        ir: Ir,
        region: Region,
        words: Vec<WordStar>,
        by_char: HashMap<String, usize>,
        leaf_index: HashMap<u64, Vec<usize>>,
        placed: HashMap<usize, (f64, f64, f64)>,
        need_overview: bool,
        encodings_pending: bool,
    }

    pub fn status() -> ApiResult {
        let (tx, rx) = mpsc::channel();
        send(Req::Status(tx))?;
        recv(rx, CALL_TIMEOUT)
    }

    pub fn probe(path: &str) -> ApiResult {
        let (tx, rx) = mpsc::channel();
        send(Req::Probe {
            path: path.to_string(),
            tx,
        })?;
        recv(rx, CALL_TIMEOUT)
    }

    pub fn open(path: &str) -> ApiResult {
        let (tx, rx) = mpsc::channel();
        send(Req::Open {
            path: path.to_string(),
            tx,
        })?;
        recv(rx, OPEN_TIMEOUT)
    }

    pub fn stars(
        session: u64,
        xmin: f64,
        xmax: f64,
        ymin: f64,
        ymax: f64,
        scale: f64,
        focus: Option<&str>,
        jump: Option<&str>,
    ) -> ApiResult {
        let (tx, rx) = mpsc::channel();
        send(Req::Stars {
            session,
            xmin,
            xmax,
            ymin,
            ymax,
            scale,
            focus: focus.filter(|s| !s.is_empty()).map(|s| s.to_string()),
            jump: jump.filter(|s| !s.is_empty()).map(|s| s.to_string()),
            tx,
        })?;
        recv(rx, CALL_TIMEOUT)
    }

    pub fn search(session: u64, q: &str) -> ApiResult {
        let (tx, rx) = mpsc::channel();
        send(Req::Search {
            session,
            q: q.to_string(),
            tx,
        })?;
        recv(rx, CALL_TIMEOUT)
    }

    pub fn query(session: u64, ch: &str) -> ApiResult {
        let (tx, rx) = mpsc::channel();
        send(Req::Query {
            session,
            ch: ch.to_string(),
            tx,
        })?;
        recv(rx, CALL_TIMEOUT)
    }

    fn send(req: Req) -> Result<(), ApiError> {
        worker_tx()
            .send(req)
            .map_err(|_| ApiError::internal("CNS worker stopped"))
    }

    fn recv(rx: mpsc::Receiver<ApiResult>, timeout: Duration) -> ApiResult {
        match rx.recv_timeout(timeout) {
            Ok(v) => v,
            Err(RecvTimeoutError::Timeout) => Err(ApiError::internal("CNS worker timed out")),
            Err(RecvTimeoutError::Disconnected) => Err(ApiError::internal("CNS worker stopped")),
        }
    }

    fn worker_tx() -> Sender<Req> {
        REQ.get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("stardust-cns".into())
                .spawn(move || worker_loop(rx))
                .expect("spawn CNS worker");
            tx
        })
        .clone()
    }

    fn worker_loop(rx: mpsc::Receiver<Req>) {
        let mut sessions: HashMap<u64, Session> = HashMap::new();
        while let Ok(req) = rx.recv() {
            match req {
                Req::Status(tx) => {
                    let _ = tx.send(Ok(json!({
                        "ok": true,
                        "python": true,
                        "reason": Json::Null,
                    })));
                }
                Req::Probe { path, tx } => {
                    let _ = tx.send(probe_path(&path));
                }
                Req::Open { path, tx } => {
                    let _ = tx.send(open_session(&mut sessions, &path));
                }
                Req::Stars {
                    session,
                    xmin,
                    xmax,
                    ymin,
                    ymax,
                    scale,
                    focus,
                    jump,
                    tx,
                } => {
                    let result = stars_of(
                        sessions.get_mut(&session),
                        xmin,
                        xmax,
                        ymin,
                        ymax,
                        scale,
                        focus.as_deref(),
                        jump.as_deref(),
                    );
                    let ok = result.is_ok();
                    let _ = tx.send(result);
                    if ok {
                        persist_after_first_stars(&mut sessions, session);
                    }
                }
                Req::Search { session, q, tx } => {
                    let _ = tx.send(search_of(sessions.get(&session), &q));
                }
                Req::Query { session, ch, tx } => {
                    let _ = tx.send(query_of(sessions.get_mut(&session), &ch));
                }
            }
        }
    }

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    }

    fn probe_path(path: &str) -> ApiResult {
        let p = PathBuf::from(path);
        match cns_world_reason(&p) {
            None => Ok(json!({
                "ok": true,
                "canInspect": true,
                "python": true,
            })),
            Some(reason) => Ok(json!({
                "ok": true,
                "canInspect": false,
                "python": true,
                "reason": reason,
            })),
        }
    }

    fn cns_world_reason(path: &Path) -> Option<String> {
        if !path.is_file() {
            return Some("not a file".into());
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.eq_ignore_ascii_case("world.sd") {
            return Some("no instance illustration".into());
        }
        let dir = path.parent()?;
        if !dir.join("cns.py").is_file() || !dir.join("luna.py").is_file() {
            return Some("CNS Python bodies missing beside world.sd".into());
        }
        if !dir.join("data").is_dir() {
            return Some("CNS data/ missing beside world.sd".into());
        }
        match Ir::from_sd_path(path) {
            Ok(ir) => {
                if ir.class("LunaStore").is_err() {
                    Some("world.sd has no LunaStore class".into())
                } else {
                    None
                }
            }
            Err(e) => Some(format!("not a StarDust layout: {e}")),
        }
    }

    fn open_session(sessions: &mut HashMap<u64, Session>, path: &str) -> ApiResult {
        let p = PathBuf::from(path);
        if let Some(reason) = cns_world_reason(&p) {
            return Err(ApiError::bad(reason));
        }
        let reuse = sessions
            .iter()
            .find(|(_, s)| s.path == p)
            .map(|(&id, _)| id);
        if let Some(id) = reuse {
            let existing = sessions.get_mut(&id).expect("reuse id");
            existing.need_overview = true;
            let n = existing.words.len();
            return Ok(json!({
                "ok": true,
                "session": id,
                "nWords": n,
                "path": existing.path.to_string_lossy(),
            }));
        }
        let ir = Ir::from_sd_path(&p).map_err(|e| ApiError::bad(format!("parse world.sd: {e}")))?;
        let lay = ir
            .layout("WorldLay")
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let mut region = Region::allocate(
            "cns_world",
            lay.inline_size() as usize,
            lay.align() as usize,
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
        let dir = p.parent().unwrap_or_else(|| Path::new("."));
        let bound = bind_store(&ir, dir)?;
        bound
            .call("import_all", &mut region, 0)
            .map_err(|e| ApiError::internal(format!("import_all: {e}")))?;
        let _ = layout_seed();
        let words = catalog(&ir, &region)?;
        let mut by_char = HashMap::new();
        for (i, w) in words.iter().enumerate() {
            by_char.insert(w.ch.clone(), i);
        }
        let leaf_index = leaf_inverted_index(&words);
        let mut placed = HashMap::with_capacity(words.len());
        for (i, w) in words.iter().enumerate() {
            placed.insert(i, (w.x, w.y, w.z));
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let n = words.len();
        sessions.insert(
            id,
            Session {
                id,
                path: p.clone(),
                ir,
                region,
                words,
                by_char,
                leaf_index,
                placed,
                need_overview: true,
                encodings_pending: true,
            },
        );
        Ok(json!({
            "ok": true,
            "session": id,
            "nWords": n,
            "path": p.to_string_lossy(),
        }))
    }

    fn bind_store<'a>(ir: &'a Ir, dir: &Path) -> Result<BoundClass<'a>, ApiError> {
        let mut cns = dir.join("cns.py");
        let mut luna = dir.join("luna.py");
        if !cns.is_file() {
            cns = workspace_root().join("examples/cns_graph/cns.py");
        }
        if !luna.is_file() {
            luna = workspace_root().join("examples/cns_graph/luna.py");
        }
        if !cns.is_file() {
            return Err(ApiError::internal("cns.py missing"));
        }
        let mut loaders = std::collections::BTreeMap::new();
        for name in ["import_all", "components_of", "stroke_order", "smoke"] {
            loaders.insert(
                name.to_string(),
                MethodLoader::Python {
                    script: cns.clone(),
                },
            );
        }
        for name in ["persist_encodings", "check_store", "check_dict", "smoke_luna"] {
            loaders.insert(
                name.to_string(),
                MethodLoader::Python {
                    script: luna.clone(),
                },
            );
        }
        BoundClass::bind(ir, "LunaStore", &loaders)
            .map_err(|e| ApiError::internal(format!("bind LunaStore: {e}")))
    }

    fn catalog(ir: &Ir, region: &Region) -> Result<Vec<WordStar>, ApiError> {
        let n = u64_field(ir, region, "n_words")?;
        let mut words = Vec::with_capacity(n as usize);
        for i in 0..n {
            let base = IrPath::new().name("words").index(i);
            let ch = slice_str(ir, region, &base.clone().name("char"))?;
            if ch.is_empty() {
                continue;
            }
            let cns = slice_str(ir, region, &base.clone().name("cns")).unwrap_or_default();
            let ids_raw = slice_str(ir, region, &base.clone().name("ids")).unwrap_or_default();
            let ids = strip_u_plus(&ids_raw);
            let seq_level = u8_field(ir, region, &base.clone().name("seq_level")).unwrap_or(0);
            let has_seq_level = u8_field(ir, region, &base.clone().name("has_seq_level")).unwrap_or(0);
            let encoding = slice_str(ir, region, &base.clone().name("encoding")).unwrap_or_default();
            let post = slice_str(ir, region, &base.clone().name("post_encoding")).unwrap_or_default();
            let leaves = ids_leaves(&ids);
            let leaf_src = if leaves.is_empty() {
                vec![ch.clone()]
            } else {
                leaves.clone()
            };
            let mut leaf_set = HashSet::new();
            let mut sx = 0.0;
            let mut sy = 0.0;
            let mut sz = 0.0;
            for leaf in &leaf_src {
                leaf_set.insert(fnv(leaf));
                let (x, y, z) = leaf_axis(leaf);
                sx += x;
                sy += y;
                sz += z;
            }
            let nleaf = leaf_src.len() as f64;
            sx /= nleaf;
            sy /= nleaf;
            sz /= nleaf;
            let (jx, jy, jz) = jitter(&ch);
            let phase = (layout_seed() as f64)
                * (std::f64::consts::TAU / (u64::MAX as f64 + 1.0));
            let a = i as f64 * GOLDEN + phase;
            let r = 22.0 * (i as f64 + 1.0).sqrt();
            words.push(WordStar {
                ch,
                cns,
                ids,
                seq_level,
                has_seq_level,
                encoding,
                post,
                leaves,
                leaf_set,
                x: sx * PLANE_SCALE + jx + r * a.cos(),
                y: sy * PLANE_SCALE + jy + r * a.sin(),
                z: sz * PLANE_SCALE * 0.35 + jz,
            });
        }
        spread_layout(&mut words);
        Ok(words)
    }

    fn leaf_inverted_index(words: &[WordStar]) -> HashMap<u64, Vec<usize>> {
        let mut inv: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, w) in words.iter().enumerate() {
            for leaf in &w.leaf_set {
                inv.entry(*leaf).or_default().push(i);
            }
        }
        inv
    }

    fn persist_after_first_stars(sessions: &mut HashMap<u64, Session>, id: u64) {
        let Some(s) = sessions.get_mut(&id) else {
            return;
        };
        if !s.encodings_pending {
            return;
        }
        s.encodings_pending = false;
        let dir = s.path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        {
            let bound = match bind_store(&s.ir, &dir) {
                Ok(b) => b,
                Err(_) => return,
            };
            if bound
                .call("persist_encodings", &mut s.region, 0)
                .is_err()
            {
                return;
            }
        }
        refresh_encodings(s);
    }

    fn refresh_encodings(s: &mut Session) {
        let n = match u64_field(&s.ir, &s.region, "n_words") {
            Ok(n) => n,
            Err(_) => return,
        };
        for i in 0..n {
            let base = IrPath::new().name("words").index(i);
            let Ok(ch) = slice_str(&s.ir, &s.region, &base.clone().name("char")) else {
                continue;
            };
            if ch.is_empty() {
                continue;
            }
            let Some(&idx) = s.by_char.get(&ch) else {
                continue;
            };
            let encoding =
                slice_str(&s.ir, &s.region, &base.clone().name("encoding")).unwrap_or_default();
            let post =
                slice_str(&s.ir, &s.region, &base.clone().name("post_encoding")).unwrap_or_default();
            if let Some(w) = s.words.get_mut(idx) {
                w.encoding = encoding;
                w.post = post;
            }
        }
    }

    fn stars_of(
        session: Option<&mut Session>,
        xmin: f64,
        xmax: f64,
        ymin: f64,
        ymax: f64,
        _scale: f64,
        focus: Option<&str>,
        jump: Option<&str>,
    ) -> ApiResult {
        let Some(s) = session else {
            return Err(ApiError::bad("CNS session missing"));
        };
        let focus = focus.filter(|c| !c.is_empty());
        let jump = jump.filter(|c| !c.is_empty());
        ensure_placed(s);
        if let Some(ch) = focus {
            let stars = neighborhood_stars(s, ch);
            return Ok(json!({
                "ok": true,
                "session": s.id,
                "mode": "neighborhood",
                "nWords": s.words.len(),
                "nPlaced": s.placed.len(),
                "stars": stars,
            }));
        }
        let overview = s.need_overview;
        if overview {
            s.need_overview = false;
        }
        let stars = if overview {
            collect_all_subset(s, jump)
        } else {
            collect_in_box(s, xmin, xmax, ymin, ymax, jump)
        };
        Ok(json!({
            "ok": true,
            "session": s.id,
            "mode": "plane",
            "nWords": s.words.len(),
            "nPlaced": s.placed.len(),
            "stars": stars,
        }))
    }

    fn layout_seed() -> u64 {
        let cur = LAYOUT_SEED.load(Ordering::Relaxed);
        if cur != 0 {
            return cur;
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let mut mixed = nanos.wrapping_mul(0x9E3779B97F4A7C15);
        if mixed == 0 {
            mixed = 1;
        }
        match LAYOUT_SEED.compare_exchange(0, mixed, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => mixed,
            Err(existing) => existing,
        }
    }

    fn ensure_placed(s: &mut Session) {
        if !s.placed.is_empty() {
            return;
        }
        let needs_layout = s.words.iter().all(|w| w.x == 0.0 && w.y == 0.0);
        if needs_layout {
            place_words(&mut s.words);
            spread_layout(&mut s.words);
        }
        for (i, w) in s.words.iter().enumerate() {
            s.placed.insert(i, (w.x, w.y, w.z));
        }
    }

    fn place_words(words: &mut [WordStar]) {
        let _ = layout_seed();
        let phase = (layout_seed() as f64)
            * (std::f64::consts::TAU / (u64::MAX as f64 + 1.0));
        for (idx, w) in words.iter_mut().enumerate() {
            let leaf_src: Vec<String> = if w.leaves.is_empty() {
                vec![w.ch.clone()]
            } else {
                w.leaves.clone()
            };
            let mut sx = 0.0;
            let mut sy = 0.0;
            let mut sz = 0.0;
            for leaf in &leaf_src {
                let (x, y, z) = leaf_axis(leaf);
                sx += x;
                sy += y;
                sz += z;
            }
            let nleaf = leaf_src.len().max(1) as f64;
            sx /= nleaf;
            sy /= nleaf;
            sz /= nleaf;
            let (jx, jy, jz) = jitter(&w.ch);
            let a = idx as f64 * GOLDEN + phase;
            let r = 22.0 * (idx as f64 + 1.0).sqrt();
            w.x = sx * PLANE_SCALE + jx + r * a.cos();
            w.y = sy * PLANE_SCALE + jy + r * a.sin();
            w.z = sz * PLANE_SCALE * 0.35 + jz;
        }
    }

    fn neighborhood_stars(s: &Session, ch: &str) -> Vec<Json> {
        let Some(&ai) = s.by_char.get(ch) else {
            return Vec::new();
        };
        let (ax, ay, az) = s
            .placed
            .get(&ai)
            .copied()
            .unwrap_or((s.words[ai].x, s.words[ai].y, s.words[ai].z));
        let mut out = Vec::new();
        out.push(star_json(&s.words[ai], "word", ax, ay, az));
        for (slot, (jac, _, i)) in scored_neighbors(s, ai)
            .into_iter()
            .take(NEIGHBOR_CAP)
            .enumerate()
        {
            let (dx, dy, dz) = nbhd_offset(&s.words[i].ch, jac, slot);
            out.push(star_json(&s.words[i], "word", ax + dx, ay + dy, az + dz));
        }
        out
    }

    fn nbhd_offset(ch: &str, j: f64, slot: usize) -> (f64, f64, f64) {
        let a = slot as f64 * GOLDEN;
        let h = fnv(ch);
        let r = 18.0 + (1.0 - j) * 72.0 + ((h % 13) as f64);
        let z = ((h.rotate_left(21) as f64) / (u64::MAX as f64 + 1.0) - 0.5) * 8.0;
        (r * a.cos(), r * a.sin(), z)
    }

    fn collect_all_subset(s: &Session, keep: Option<&str>) -> Vec<Json> {
        let mut idx: Vec<usize> = s.placed.keys().copied().collect();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        if let Some(ch) = keep {
            if let Some(&i) = s.by_char.get(ch) {
                if let Some(j) = placed_json(s, i) {
                    seen.insert(i);
                    out.push(j);
                }
            }
        }
        if idx.len() <= STAR_CAP {
            idx.sort_unstable();
            for i in idx {
                if seen.insert(i) {
                    if let Some(j) = placed_json(s, i) {
                        out.push(j);
                    }
                }
            }
            return out;
        }
        idx.sort_by(|a, b| {
            mix_star(*a)
                .cmp(&mix_star(*b))
                .then_with(|| a.cmp(b))
        });
        for i in idx {
            if out.len() >= STAR_CAP {
                break;
            }
            if seen.insert(i) {
                if let Some(j) = placed_json(s, i) {
                    out.push(j);
                }
            }
        }
        out
    }

    fn scored_neighbors(s: &Session, ai: usize) -> Vec<(f64, u32, usize)> {
        let aw = &s.words[ai];
        let mut cand: HashSet<usize> = HashSet::new();
        for leaf in &aw.leaf_set {
            if let Some(list) = s.leaf_index.get(leaf) {
                for &i in list {
                    if i != ai {
                        cand.insert(i);
                    }
                }
            }
        }
        let mut scored: Vec<(f64, u32, usize)> = Vec::new();
        for i in cand {
            let w = &s.words[i];
            let shared = aw.leaf_set.intersection(&w.leaf_set).count() as u32;
            if shared < MIN_SHARED_LEAVES {
                continue;
            }
            let j = leaf_jaccard(&aw.leaf_set, &w.leaf_set);
            scored.push((j, shared, i));
        }
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        scored
    }

    fn collect_in_box(
        s: &Session,
        xmin: f64,
        xmax: f64,
        ymin: f64,
        ymax: f64,
        keep: Option<&str>,
    ) -> Vec<Json> {
        let mut pts: Vec<(f64, f64, usize)> = s
            .placed
            .iter()
            .filter_map(|(&i, &(x, y, _))| {
                if x < xmin || x > xmax || y < ymin || y > ymax {
                    None
                } else {
                    Some((x, y, i))
                }
            })
            .collect();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        if let Some(ch) = keep {
            if let Some(&i) = s.by_char.get(ch) {
                if let Some(j) = placed_json(s, i) {
                    seen.insert(i);
                    out.push(j);
                }
            }
        }
        if pts.len() <= STAR_CAP {
            pts.sort_unstable_by_key(|p| p.2);
            for (_, _, i) in pts {
                if seen.insert(i) {
                    if let Some(j) = placed_json(s, i) {
                        out.push(j);
                    }
                }
            }
            return out;
        }
        pts.sort_by(|a, b| {
            mix_star(a.2)
                .cmp(&mix_star(b.2))
                .then_with(|| a.2.cmp(&b.2))
        });
        for (_, _, i) in pts {
            if out.len() >= STAR_CAP {
                break;
            }
            if seen.insert(i) {
                if let Some(j) = placed_json(s, i) {
                    out.push(j);
                }
            }
        }
        out
    }

    fn mix_star(i: usize) -> u64 {
        (i as u64)
            .wrapping_mul(0x9E3779B97F4A7C15)
            .rotate_left(21)
    }

    fn placed_json(s: &Session, i: usize) -> Option<Json> {
        let w = s.words.get(i)?;
        let &(x, y, z) = s.placed.get(&i)?;
        Some(star_json(w, "word", x, y, z))
    }

    fn star_json(w: &WordStar, role: &str, x: f64, y: f64, z: f64) -> Json {
        json!({
            "name": w.ch,
            "path": format!("cns:word:{}", w.ch),
            "kind": "file",
            "role": role,
            "cns": w.cns,
            "ids": w.ids,
            "encoding": w.encoding,
            "post": w.post,
            "x": x,
            "y": y,
            "z": z,
            "isText": false,
        })
    }

    fn search_of(session: Option<&Session>, q: &str) -> ApiResult {
        let Some(s) = session else {
            return Err(ApiError::bad("CNS session missing"));
        };
        let q = q.trim();
        if q.is_empty() {
            return Ok(json!({ "ok": true, "hits": [] }));
        }
        let ql = q.to_ascii_lowercase();
        let mut hits: Vec<(i32, Json)> = Vec::new();
        for (i, w) in s.words.iter().enumerate() {
            let enc = w.encoding.to_ascii_lowercase();
            let post = w.post.to_ascii_lowercase();
            let cns = w.cns.to_ascii_lowercase();
            let rank = if w.ch == q {
                400
            } else if enc == ql {
                300
            } else if post == ql {
                280
            } else if cns == ql {
                260
            } else if !enc.is_empty() && enc.starts_with(&ql) {
                180
            } else if !post.is_empty() && post.starts_with(&ql) {
                170
            } else if !cns.is_empty() && cns.starts_with(&ql) {
                160
            } else if w.ch.contains(q) {
                120
            } else if enc.contains(&ql) || post.contains(&ql) || cns.contains(&ql) {
                80
            } else {
                0
            };
            if rank > 0 {
                let (x, y) = s
                    .placed
                    .get(&i)
                    .map(|&(x, y, _)| (x, y))
                    .unwrap_or((0.0, 0.0));
                hits.push((
                    rank,
                    json!({
                        "name": w.ch,
                        "path": format!("cns:word:{}", w.ch),
                        "kind": "cns",
                        "parent": "cns:plane",
                        "cns": w.cns,
                        "encoding": w.encoding,
                        "post": w.post,
                        "x": x,
                        "y": y,
                    }),
                ));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0));
        let hits: Vec<Json> = hits.into_iter().take(40).map(|(_, h)| h).collect();
        Ok(json!({ "ok": true, "hits": hits }))
    }

    fn query_of(session: Option<&mut Session>, ch: &str) -> ApiResult {
        let Some(s) = session else {
            return Err(ApiError::bad("CNS session missing"));
        };
        let Some(i) = s.by_char.get(ch).copied() else {
            return Err(ApiError::bad(format!("unknown 字 {ch}")));
        };
        let meta = {
            let w = &s.words[i];
            json!({
                "char": w.ch,
                "cns": w.cns,
                "ids": w.ids,
                "seqLevel": w.seq_level,
                "hasSeqLevel": w.has_seq_level != 0,
                "encoding": w.encoding,
                "post": w.post,
                "leaves": w.leaves,
            })
        };
        let dir = s.path.parent().unwrap_or_else(|| Path::new("."));
        let bound = bind_store(&s.ir, dir)?;
        intern_query(&s.ir, &mut s.region, ch)?;
        bound
            .call("stroke_order", &mut s.region, 0)
            .map_err(|e| ApiError::internal(format!("stroke_order: {e}")))?;
        let strokes = slice_str(&s.ir, &s.region, &IrPath::new().name("result")).unwrap_or_default();
        intern_query(&s.ir, &mut s.region, ch)?;
        bound
            .call("components_of", &mut s.region, 0)
            .map_err(|e| ApiError::internal(format!("components_of: {e}")))?;
        let components = slice_str(&s.ir, &s.region, &IrPath::new().name("result")).unwrap_or_default();
        Ok(json!({
            "ok": true,
            "detail": meta,
            "strokeOrder": strokes,
            "componentsOf": components,
        }))
    }

    fn intern_query(ir: &Ir, region: &mut Region, s: &str) -> Result<(), ApiError> {
        let used = u64_field(ir, region, "blob_used")?;
        let blob_base = field_offset(ir, "WorldLay", "blob")?;
        let bytes = s.as_bytes();
        let cap = match ir.layout("WorldLay") {
            Ok(Type::Struct(st)) => st
                .fields
                .iter()
                .find(|f| f.name == "blob")
                .and_then(|f| match &f.ty {
                    Type::Array(a) => Some(a.count),
                    _ => None,
                })
                .unwrap_or(0),
            _ => 0,
        };
        if used + bytes.len() as u64 > cap {
            return Err(ApiError::internal("CNS blob full"));
        }
        let elems: Vec<Value> = bytes.iter().copied().map(Value::U8).collect();
        ir.store(
            "WorldLay",
            region,
            &IrPath::new().name("query"),
            &Value::Slice {
                offset: blob_base + used,
                elems,
            },
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
        ir.store(
            "WorldLay",
            region,
            &IrPath::new().name("blob_used"),
            &Value::U64(used + bytes.len() as u64),
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(())
    }

    fn field_offset(ir: &Ir, layout: &str, name: &str) -> Result<u64, ApiError> {
        match ir.layout(layout) {
            Ok(Type::Struct(st)) => st
                .fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.offset)
                .ok_or_else(|| ApiError::internal(format!("missing field {name}"))),
            _ => Err(ApiError::internal("WorldLay is not a struct")),
        }
    }

    fn u64_field(ir: &Ir, region: &Region, name: &str) -> Result<u64, ApiError> {
        match ir.retrieve("WorldLay", region, &IrPath::new().name(name)) {
            Ok(Value::U64(v)) => Ok(v),
            Ok(_) => Err(ApiError::internal(format!("{name} not u64"))),
            Err(e) => Err(ApiError::internal(e.to_string())),
        }
    }

    fn u8_field(ir: &Ir, region: &Region, path: &IrPath) -> Result<u8, ApiError> {
        match ir.retrieve("WorldLay", region, path) {
            Ok(Value::U8(v)) => Ok(v),
            Ok(_) => Err(ApiError::internal("expected u8")),
            Err(e) => Err(ApiError::internal(e.to_string())),
        }
    }

    fn slice_str(ir: &Ir, region: &Region, path: &IrPath) -> Result<String, ApiError> {
        match ir.retrieve("WorldLay", region, path) {
            Ok(Value::Slice { elems, .. }) => {
                let mut bytes = Vec::with_capacity(elems.len());
                for e in elems {
                    if let Value::U8(b) = e {
                        bytes.push(b);
                    }
                }
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            }
            Ok(_) => Ok(String::new()),
            Err(e) => Err(ApiError::internal(e.to_string())),
        }
    }

    fn strip_u_plus(ids: &str) -> String {
        let chars: Vec<char> = ids.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '('
                && i + 2 < chars.len()
                && (chars[i + 1] == 'U' || chars[i + 1] == 'u')
                && chars[i + 2] == '+'
            {
                let mut j = i + 3;
                while j < chars.len() && chars[j] != ')' {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ')' {
                    i = j + 1;
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    fn ids_leaves(ids: &str) -> Vec<String> {
        let mut out = Vec::new();
        let chars: Vec<char> = ids.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if IDC.contains(&c) {
                i += 1;
                continue;
            }
            if c == '&' {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != ';' {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ';' {
                    j += 1;
                }
                out.push(chars[i..j].iter().collect());
                i = j;
                continue;
            }
            if c.is_whitespace() {
                i += 1;
                continue;
            }
            out.push(c.to_string());
            i += 1;
        }
        out
    }

    fn leaf_jaccard(a: &HashSet<u64>, b: &HashSet<u64>) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        let inter = a.intersection(b).count();
        if inter == 0 {
            return 0.0;
        }
        let union = a.len() + b.len() - inter;
        inter as f64 / union.max(1) as f64
    }

    fn leaf_axis(leaf: &str) -> (f64, f64, f64) {
        let h = fnv(leaf).wrapping_add(layout_seed().rotate_left(7));
        let a = (h as f64) * (std::f64::consts::TAU / (u64::MAX as f64 + 1.0));
        let b = ((h.rotate_left(21)) as f64) * (std::f64::consts::TAU / (u64::MAX as f64 + 1.0));
        (a.cos(), a.sin(), b.cos())
    }

    fn jitter(ch: &str) -> (f64, f64, f64) {
        let h = fnv(ch);
        let x = ((h % 1000) as f64 / 1000.0 - 0.5) * 96.0;
        let y = (((h / 1000) % 1000) as f64 / 1000.0 - 0.5) * 96.0;
        let z = (((h / 1_000_000) % 1000) as f64 / 1000.0 - 0.5) * 24.0;
        (x, y, z)
    }

    fn spread_layout(words: &mut [WordStar]) {
        let n = words.len();
        if n < 2 {
            return;
        }
        let mut inv: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, w) in words.iter().enumerate() {
            for leaf in &w.leaf_set {
                inv.entry(*leaf).or_default().push(i);
            }
        }
        let mut vx = vec![0.0; n];
        let mut vy = vec![0.0; n];
        for _ in 0..16 {
            vx.fill(0.0);
            vy.fill(0.0);
            for i in 0..n {
                let mut pulls = 0usize;
                for leaf in &words[i].leaf_set {
                    if pulls >= 8 {
                        break;
                    }
                    let Some(mems) = inv.get(leaf) else {
                        continue;
                    };
                    for &j in mems.iter().take(12) {
                        if i == j {
                            continue;
                        }
                        let dx = words[j].x - words[i].x;
                        let dy = words[j].y - words[i].y;
                        vx[i] += dx * 0.018;
                        vy[i] += dy * 0.018;
                        pulls += 1;
                    }
                }
            }
            let cell = 110.0;
            let mut bins: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
            for i in 0..n {
                let gx = (words[i].x / cell).floor() as i32;
                let gy = (words[i].y / cell).floor() as i32;
                bins.entry((gx, gy)).or_default().push(i);
            }
            for i in 0..n {
                let gx = (words[i].x / cell).floor() as i32;
                let gy = (words[i].y / cell).floor() as i32;
                for ox in -1..=1 {
                    for oy in -1..=1 {
                        let Some(mems) = bins.get(&(gx + ox, gy + oy)) else {
                            continue;
                        };
                        for &j in mems {
                            if j <= i {
                                continue;
                            }
                            let dx = words[i].x - words[j].x;
                            let dy = words[i].y - words[j].y;
                            let d2 = dx * dx + dy * dy + 36.0;
                            let f = 1400.0 / d2;
                            vx[i] += dx * f;
                            vy[i] += dy * f;
                            vx[j] -= dx * f;
                            vy[j] -= dy * f;
                        }
                    }
                }
            }
            for i in 0..n {
                let dx = vx[i].clamp(-55.0, 55.0);
                let dy = vy[i].clamp(-55.0, 55.0);
                words[i].x += dx;
                words[i].y += dy;
            }
        }
    }

    fn fnv(s: &str) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in s.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}
