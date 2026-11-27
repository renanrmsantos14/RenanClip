//! Motor: servidor TCP, discovery, pareamento, sessões cifradas e sincronização.
//! Sem interface: o app hospedeiro recebe `Event`s e chama os métodos de `Engine`.

use crate::crypto::*;
use crate::discovery::{Discovery, Nearby};
use crate::frame::*;
use crate::pairing;
use crate::proto::*;
use crate::settings::Settings;
use crate::store::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct Config {
    pub data_dir: PathBuf,
    pub device_name: Option<String>,
    /// 0 = porta automática.
    pub port: u16,
    pub mdns: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct PeerView {
    pub id: String,
    pub name: String,
    pub original_name: String,
    pub alias: String,
    pub online: bool,
    pub nearby: bool,
    pub addr: String,
    pub last_seen: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct NearbyView {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub paired: bool,
}

enum Out {
    Msg(Msg),
    Chunk { id: [u8; 16], off: u32, data: Vec<u8> },
    Close,
}

struct Conn {
    cid: u64,
    is_client: bool,
    tx: Sender<Out>,
    shut: TcpStream,
}

struct Asm {
    meta: ClipMeta,
    buf: Vec<u8>,
}

struct Inner {
    store: Store,
    my_id: [u8; 16],
    my_hex: String,
    port: u16,
    settings: RwLock<Settings>,
    events: Box<dyn Fn(Event) + Send + Sync>,
    conns: Mutex<HashMap<String, Conn>>,
    nearby: Mutex<HashMap<String, Nearby>>,
    dialing: Mutex<HashSet<String>>,
    pairing_until: AtomicI64,
    waiters: Mutex<HashMap<u64, Sender<bool>>>,
    next_id: AtomicU64,
    last_applied: Mutex<Option<(String, Instant)>>,
    stop: AtomicBool,
    disc: Mutex<Option<Discovery>>,
}

#[derive(Clone)]
pub struct Engine(Arc<Inner>);

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "RenanClip".into())
}

impl Engine {
    pub fn start(cfg: Config, events: impl Fn(Event) + Send + Sync + 'static) -> Result<Engine> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let store = Store::open(&cfg.data_dir.join("renanclip.db"))?;
        let my_hex = match store.setting("device_id") {
            Some(h) if h.len() == 32 => h,
            _ => {
                let h = hex(&rand_bytes::<16>());
                store.set_setting("device_id", &h)?;
                h
            }
        };
        let my_id: [u8; 16] = unhex_n(&my_hex)?;
        let mut settings: Settings = store.setting("settings").and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        if settings.device_name.is_empty() {
            settings.device_name = cfg.device_name.clone().unwrap_or_else(hostname);
        }
        let listener = TcpListener::bind(("0.0.0.0", cfg.port))?;
        let port = listener.local_addr()?.port();
        let inner = Arc::new(Inner {
            store,
            my_id,
            my_hex: my_hex.clone(),
            port,
            settings: RwLock::new(settings.clone()),
            events: Box::new(events),
            conns: Mutex::new(HashMap::new()),
            nearby: Mutex::new(HashMap::new()),
            dialing: Mutex::new(HashSet::new()),
            pairing_until: AtomicI64::new(0),
            waiters: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            last_applied: Mutex::new(None),
            stop: AtomicBool::new(false),
            disc: Mutex::new(None),
        });
        let e = Engine(inner.clone());
        e.save_settings(&settings);
        let _ = inner.purge();

        let me = inner.clone();
        std::thread::Builder::new().name("accept".into()).spawn(move || {
            for s in listener.incoming() {
                if me.stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(s) = s {
                    let me = me.clone();
                    let _ = std::thread::Builder::new().name("inbound".into()).spawn(move || me.handle_inbound(s));
                }
            }
        })?;

        if cfg.mdns {
            let (up, down) = (inner.clone(), inner.clone());
            match Discovery::start(
                &my_hex,
                &settings.device_name,
                port,
                move |n| {
                    up.nearby.lock().unwrap().insert(n.id.clone(), n.clone());
                    (up.events)(Event::Peers);
                    if up.store.peer_key(&n.id).is_some() {
                        up.dial_async(&n.id);
                    }
                },
                move |id| {
                    down.nearby.lock().unwrap().remove(&id);
                    (down.events)(Event::Peers);
                },
            ) {
                Ok(d) => *inner.disc.lock().unwrap() = Some(d),
                Err(err) => eprintln!("renanclip: mDNS indisponível ({err}); use conexão manual"),
            }
        }

        let me = inner.clone();
        std::thread::Builder::new().name("maint".into()).spawn(move || {
            let mut tick = 0u64;
            while !me.stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(5));
                tick += 1;
                for p in me.store.peers() {
                    me.dial_async(&p.id);
                }
                if tick % 720 == 0 {
                    let _ = me.purge();
                }
            }
        })?;
        Ok(e)
    }

    pub fn shutdown(&self) {
        self.0.stop.store(true, Ordering::Relaxed);
        if let Some(d) = self.0.disc.lock().unwrap().take() {
            d.stop();
        }
        for (_, c) in self.0.conns.lock().unwrap().drain() {
            let _ = c.tx.send(Out::Close);
            let _ = c.shut.shutdown(std::net::Shutdown::Both);
        }
        let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], self.0.port)), Duration::from_millis(300));
    }

    pub fn my_id(&self) -> &str {
        &self.0.my_hex
    }
    pub fn port(&self) -> u16 {
        self.0.port
    }
    pub fn store(&self) -> &Store {
        &self.0.store
    }

    // ---------- configurações ----------
    pub fn settings(&self) -> Settings {
        self.0.settings.read().unwrap().clone()
    }
    fn save_settings(&self, s: &Settings) {
        if let Ok(j) = serde_json::to_string(s) {
            let _ = self.0.store.set_setting("settings", &j);
        }
    }
    pub fn update_settings(&self, s: Settings) -> Settings {
        let s = s.clamp();
        self.save_settings(&s);
        *self.0.settings.write().unwrap() = s.clone();
        let _ = self.0.purge();
        (self.0.events)(Event::History);
        s
    }

    // ---------- histórico ----------
    /// Registra uma cópia local e a envia aos aparelhos conectados. `None` se ignorada.
    pub fn local_copy(&self, c: LocalCopy) -> Option<String> {
        let s = self.settings();
        if s.paused || c.sensitive || c.source_app.as_deref().is_some_and(|a| s.is_ignored(a)) {
            return None;
        }
        let cap = (s.max_blob_mb.max(1) as usize) * 1024 * 1024;
        let (bytes, text, blob): (Vec<u8>, Option<String>, Option<Vec<u8>>) = match c.kind.as_str() {
            "text" => {
                let t = c.text?;
                if t.trim().is_empty() || t.len() > MAX_TEXT {
                    return None;
                }
                (t.clone().into_bytes(), Some(t), None)
            }
            "image" if s.sync_images => {
                let b = c.bytes?;
                if b.is_empty() || b.len() > cap {
                    return None;
                }
                (b.clone(), None, Some(b))
            }
            "file" if s.sync_files => {
                let b = c.bytes?;
                if b.is_empty() || b.len() > cap || c.name.is_empty() {
                    return None;
                }
                (b.clone(), None, Some(b))
            }
            _ => return None,
        };
        let name = if c.kind == "file" { c.name.clone() } else { String::new() };
        let id = content_id(&c.kind, &name, &bytes);
        if self.0.recently_applied(&id) {
            return None;
        }
        let item = NewItem { kind: c.kind, name, mime: c.mime, text, origin: self.0.my_hex.clone() };
        let r = self.0.store.upsert(&id, &item, bytes.len() as i64, now_ms(), blob.as_deref()).ok()?;
        if r == Upsert::Unchanged {
            return None;
        }
        (self.0.events)(Event::History);
        let _ = self.0.purge();
        self.0.broadcast_item(&id, true);
        Some(id)
    }

    pub fn history(&self, query: &str, kind: Option<&str>, limit: usize) -> Vec<Item> {
        self.0.store.history(query, kind, limit).unwrap_or_default()
    }

    pub fn payload(&self, id: &str) -> Option<Payload> {
        let it = self.0.store.get(id).ok().flatten()?;
        match it.kind.as_str() {
            "text" => self.0.store.full_text(id).map(Payload::Text),
            "image" => self.0.store.blob(id).map(|png| Payload::Image { png }),
            "file" => self.0.store.blob(id).map(|bytes| Payload::File { name: it.name, mime: it.mime, bytes }),
            _ => None,
        }
    }

    /// O app hospedeiro chama isto depois de colocar o item na área de transferência por
    /// ação do usuário, para o observador não reenviar o mesmo conteúdo como "nova cópia".
    pub fn mark_applied(&self, id: &str) {
        *self.0.last_applied.lock().unwrap() = Some((id.to_string(), Instant::now()));
    }

    /// Reenvia um item do histórico aos aparelhos (ao escolher um item antigo, ele sobe ao topo).
    pub fn touch(&self, id: &str) {
        if let Ok(Some(it)) = self.0.store.get(id) {
            let _ = self.0.store.upsert(id, &NewItem { kind: it.kind.clone(), name: it.name.clone(), mime: it.mime.clone(), text: None, origin: self.0.my_hex.clone() }, it.size, now_ms(), None);
            (self.0.events)(Event::History);
            self.0.broadcast_item(id, true);
        }
    }

    pub fn pin(&self, id: &str, pinned: bool) {
        let _ = self.0.store.set_pinned(id, pinned);
        (self.0.events)(Event::History);
    }
    pub fn delete(&self, id: &str) {
        let _ = self.0.store.delete(id, now_ms());
        (self.0.events)(Event::History);
    }
    pub fn clear(&self) -> usize {
        let n = self.0.store.clear_unpinned(now_ms()).unwrap_or(0);
        (self.0.events)(Event::History);
        n
    }

    // ---------- aparelhos ----------
    pub fn peers(&self) -> Vec<PeerView> {
        let conns = self.0.conns.lock().unwrap();
        let near = self.0.nearby.lock().unwrap();
        self.0
            .store
            .peers()
            .into_iter()
            .map(|p| PeerView {
                online: conns.contains_key(&p.id),
                nearby: near.contains_key(&p.id),
                name: p.display().to_string(),
                original_name: p.name,
                alias: p.alias,
                addr: p.addr,
                last_seen: p.last_seen,
                id: p.id,
            })
            .collect()
    }

    pub fn nearby(&self) -> Vec<NearbyView> {
        let paired: HashSet<String> = self.0.store.peers().into_iter().map(|p| p.id).collect();
        let mut v: Vec<_> = self
            .0
            .nearby
            .lock()
            .unwrap()
            .values()
            .map(|n| NearbyView { id: n.id.clone(), name: n.name.clone(), addr: n.addrs.first().map(|a| a.to_string()).unwrap_or_default(), paired: paired.contains(&n.id) })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn rename_peer(&self, id: &str, alias: &str) {
        let _ = self.0.store.rename_peer(id, alias);
        (self.0.events)(Event::Peers);
    }

    /// Revoga o aparelho: apaga a chave e, se estiver online, avisa o outro lado.
    pub fn revoke_peer(&self, id: &str) {
        if let Some(c) = self.0.conns.lock().unwrap().remove(id) {
            let _ = c.tx.send(Out::Msg(Msg::Bye { revoked: true }));
            let _ = c.tx.send(Out::Close);
        }
        let _ = self.0.store.remove_peer(id);
        (self.0.events)(Event::Peers);
    }

    // ---------- pareamento ----------
    /// Aceita pareamentos de entrada por `secs` segundos.
    pub fn open_pairing(&self, secs: i64) {
        self.0.pairing_until.store(now_ms() + secs.max(0) * 1000, Ordering::Relaxed);
    }

    pub fn pair_decide(&self, pair_id: u64, accept: bool) {
        if let Some(w) = self.0.waiters.lock().unwrap().remove(&pair_id) {
            let _ = w.send(accept);
        }
    }

    /// Inicia o pareamento com um aparelho em `addr`. Devolve o id do pareamento (eventos `Pair*`).
    pub fn pair_with(&self, addr: SocketAddr) -> u64 {
        let me = self.0.clone();
        let pid = me.next_id.fetch_add(1, Ordering::Relaxed);
        let _ = std::thread::Builder::new().name("pair-out".into()).spawn(move || {
            let r = (|| -> Result<(pairing::Paired, SocketAddr)> {
                let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(4))?;
                s.set_read_timeout(Some(Duration::from_secs(135)))?;
                write_preamble(&mut s, MODE_PAIR)?;
                let name = me.settings.read().unwrap().device_name.clone();
                let p = pairing::initiate(&mut s, &me.my_id, &name, &mut |_, pn, sas| me.ask_user(pid, pn, sas, false))?;
                Ok((p, addr))
            })();
            me.finish_pair(pid, r.map(|(p, a)| (p, Some(a))));
        });
        pid
    }

    /// Conexão manual (rede sem mDNS): guarda o endereço de um aparelho já pareado.
    pub fn set_peer_addr(&self, id: &str, addr: &str) {
        let _ = self.0.store.touch_peer(id, addr, 0);
        self.0.dial_async(id);
    }

    pub fn online_count(&self) -> usize {
        self.0.conns.lock().unwrap().len()
    }
}

impl Inner {
    fn purge(&self) -> Result<usize> {
        let s = self.settings.read().unwrap().clone();
        let n = self.store.purge(now_ms(), s.retention_days, s.store_cap_mb * 1024 * 1024)?;
        if n > 0 {
            (self.events)(Event::History);
        }
        Ok(n)
    }

    fn recently_applied(&self, id: &str) -> bool {
        matches!(&*self.last_applied.lock().unwrap(), Some((a, t)) if a == id && t.elapsed() < Duration::from_secs(4))
    }

    fn ask_user(&self, pid: u64, peer_name: &str, sas: &str, inbound: bool) -> bool {
        let (tx, rx) = channel();
        self.waiters.lock().unwrap().insert(pid, tx);
        (self.events)(Event::PairPrompt { pair_id: pid, peer_name: peer_name.into(), sas: sas.into(), inbound });
        let ok = rx.recv_timeout(Duration::from_secs(120)).unwrap_or(false);
        self.waiters.lock().unwrap().remove(&pid);
        ok
    }

    fn finish_pair(self: &Arc<Self>, pid: u64, r: Result<(pairing::Paired, Option<SocketAddr>)>) {
        match r {
            Ok((p, addr)) => {
                let id = hex(&p.peer_id);
                let _ = self.store.add_peer(&id, &p.peer_name, &p.key, &addr.map(|a| a.to_string()).unwrap_or_default(), now_ms());
                (self.events)(Event::PairDone { pair_id: pid, ok: true, peer_name: p.peer_name, error: String::new() });
                (self.events)(Event::Peers);
                self.dial_async(&id);
            }
            Err(e) => (self.events)(Event::PairDone { pair_id: pid, ok: false, peer_name: String::new(), error: e.to_string() }),
        }
    }

    // ---------- entrada ----------
    fn handle_inbound(self: Arc<Self>, mut s: TcpStream) {
        let _ = s.set_nodelay(true);
        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
        let Ok(mode) = read_preamble(&mut s) else { return };
        match mode {
            MODE_PAIR => {
                if now_ms() > self.pairing_until.load(Ordering::Relaxed) {
                    return; // ninguém abriu o pareamento: ignora
                }
                let _ = s.set_read_timeout(Some(Duration::from_secs(135)));
                let pid = self.next_id.fetch_add(1, Ordering::Relaxed);
                let name = self.settings.read().unwrap().device_name.clone();
                let r = pairing::respond(&mut s, &self.my_id, &name, &mut |_, pn, sas| self.ask_user(pid, pn, sas, true));
                self.finish_pair(pid, r.map(|p| (p, None)));
            }
            MODE_SESSION => {
                let Ok(raw) = read_frame(&mut s, 512) else { return };
                let Ok(Hello::S1 { id, n }) = serde_json::from_slice::<Hello>(&raw) else { return };
                let Some(key) = self.store.peer_key(&id) else { return };
                let Ok(nc) = unhex_n::<16>(&n) else { return };
                let ns = rand_bytes::<16>();
                let hello = serde_json::to_vec(&Hello::S2 { id: self.my_hex.clone(), n: hex(&ns) }).unwrap();
                if write_frame(&mut s, &hello).is_err() {
                    return;
                }
                let (c2s, s2c) = session_keys(&key, &nc, &ns);
                self.run_session(s, id, Seal::new(&s2c, Dir::ServerToClient), Seal::new(&c2s, Dir::ClientToServer), false, None);
            }
            _ => {}
        }
    }

    // ---------- saída ----------
    fn dial_async(self: &Arc<Self>, peer: &str) {
        if self.stop.load(Ordering::Relaxed) || self.conns.lock().unwrap().contains_key(peer) {
            return;
        }
        if !self.dialing.lock().unwrap().insert(peer.to_string()) {
            return;
        }
        let me = self.clone();
        let peer = peer.to_string();
        let _ = std::thread::Builder::new().name("dial".into()).spawn(move || {
            let _ = me.dial(&peer);
            me.dialing.lock().unwrap().remove(&peer);
        });
    }

    fn dial(self: &Arc<Self>, peer: &str) -> Result<()> {
        let key = self.store.peer_key(peer).ok_or(Error::Proto("aparelho desconhecido".into()))?;
        let mut addrs: Vec<SocketAddr> = self.nearby.lock().unwrap().get(peer).map(|n| n.addrs.clone()).unwrap_or_default();
        if let Some(p) = self.store.peers().into_iter().find(|p| p.id == peer) {
            if let Ok(a) = p.addr.parse::<SocketAddr>() {
                if !addrs.contains(&a) {
                    addrs.push(a);
                }
            }
        }
        let mut last = Error::Proto("sem endereço".into());
        for a in addrs {
            match self.dial_one(peer, &key, a) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn dial_one(self: &Arc<Self>, peer: &str, key: &[u8; 32], addr: SocketAddr) -> Result<()> {
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(Duration::from_secs(6)))?;
        write_preamble(&mut s, MODE_SESSION)?;
        let nc = rand_bytes::<16>();
        write_frame(&mut s, &serde_json::to_vec(&Hello::S1 { id: self.my_hex.clone(), n: hex(&nc) })?)?;
        let Hello::S2 { id, n } = serde_json::from_slice(&read_frame(&mut s, 512)?)? else { return Err(Error::Proto("esperava s2".into())) };
        if id != peer {
            return Err(Error::Proto("aparelho diferente do esperado".into()));
        }
        let ns: [u8; 16] = unhex_n(&n)?;
        let (c2s, s2c) = session_keys(key, &nc, &ns);
        self.run_session(s, id, Seal::new(&c2s, Dir::ClientToServer), Seal::new(&s2c, Dir::ServerToClient), true, Some(addr));
        Ok(())
    }

    // ---------- sessão ----------
    fn run_session(self: &Arc<Self>, stream: TcpStream, peer: String, mut tx_seal: Seal, mut rx_seal: Seal, is_client: bool, addr: Option<SocketAddr>) {
        let Ok(mut rd) = stream.try_clone() else { return };
        let Ok(mut wr) = stream.try_clone() else { return };
        let _ = rd.set_read_timeout(Some(Duration::from_secs(45)));
        let cid = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx): (Sender<Out>, Receiver<Out>) = channel();

        {
            let mut conns = self.conns.lock().unwrap();
            if let Some(old) = conns.get(&peer) {
                // Duas conexões simultâneas: fica a aberta pelo aparelho de menor id (regra igual nos dois lados).
                let new_client = if is_client { &self.my_hex } else { &peer };
                let old_client = if old.is_client { &self.my_hex } else { &peer };
                if new_client > old_client {
                    return;
                }
                let _ = old.tx.send(Out::Close);
                let _ = old.shut.shutdown(std::net::Shutdown::Both);
            }
            conns.insert(peer.clone(), Conn { cid, is_client, tx: tx.clone(), shut: stream });
        }
        let _ = self.store.touch_peer(&peer, &addr.map(|a| a.to_string()).unwrap_or_default(), now_ms());
        (self.events)(Event::Peers);

        let wtx = std::thread::Builder::new().name("conn-write".into()).spawn(move || {
            loop {
                let out = match rx.recv_timeout(Duration::from_secs(15)) {
                    Ok(o) => o,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Out::Msg(Msg::Ping {}),
                    Err(_) => break,
                };
                let plain = match out {
                    Out::Close => break,
                    Out::Msg(m) => {
                        let mut v = vec![T_JSON];
                        v.extend(serde_json::to_vec(&m).unwrap_or_default());
                        v
                    }
                    Out::Chunk { id, off, data } => {
                        let mut v = Vec::with_capacity(21 + data.len());
                        v.push(T_CHUNK);
                        v.extend_from_slice(&id);
                        v.extend_from_slice(&off.to_be_bytes());
                        v.extend_from_slice(&data);
                        v
                    }
                };
                let Ok(ct) = tx_seal.seal(&plain) else { break };
                if write_frame(&mut wr, &ct).is_err() {
                    break;
                }
            }
            let _ = wr.shutdown(std::net::Shutdown::Both);
        });

        // inventário inicial: o outro lado pede o que lhe falta
        let inv = self.store.inventory(INVENTORY).unwrap_or_default();
        let _ = tx.send(Out::Msg(Msg::Inv { items: inv }));

        let mut asm: HashMap<[u8; 16], Asm> = HashMap::new();
        loop {
            let ct = match read_frame(&mut rd, MAX_FRAME + 64) {
                Ok(c) => c,
                Err(_) => break,
            };
            let Ok(plain) = rx_seal.open(&ct) else { break };
            if plain.is_empty() {
                break;
            }
            let keep = match plain[0] {
                T_JSON => match serde_json::from_slice::<Msg>(&plain[1..]) {
                    Ok(m) => self.on_msg(&peer, &tx, &mut asm, m),
                    Err(_) => false,
                },
                T_CHUNK if plain.len() > 21 => self.on_chunk(&peer, &mut asm, &plain),
                _ => false,
            };
            if !keep {
                break;
            }
        }
        let _ = tx.send(Out::Close);
        if let Some(h) = wtx.ok() {
            let _ = h.join();
        }
        let mut conns = self.conns.lock().unwrap();
        if conns.get(&peer).is_some_and(|c| c.cid == cid) {
            conns.remove(&peer);
            drop(conns);
            (self.events)(Event::Peers);
        }
    }

    fn on_msg(self: &Arc<Self>, peer: &str, tx: &Sender<Out>, asm: &mut HashMap<[u8; 16], Asm>, m: Msg) -> bool {
        match m {
            Msg::Ping {} => {
                let _ = tx.send(Out::Msg(Msg::Pong {}));
            }
            Msg::Pong {} => {}
            Msg::Bye { revoked } => {
                if revoked {
                    let _ = self.store.remove_peer(peer);
                    (self.events)(Event::Peers);
                }
                return false;
            }
            Msg::Inv { items } => {
                let want: Vec<String> = items
                    .into_iter()
                    .take(INVENTORY * 2)
                    .filter(|(id, ts)| id.len() == 32 && self.store.has_row(id).map_or(true, |mine| *ts > mine))
                    .map(|(id, _)| id)
                    .collect();
                if !want.is_empty() {
                    let _ = tx.send(Out::Msg(Msg::Want { ids: want }));
                }
            }
            Msg::Want { ids } => {
                for id in ids.iter().take(INVENTORY * 2) {
                    self.send_item(tx, id, false);
                }
            }
            Msg::Clip(meta) => return self.on_clip(peer, asm, meta),
        }
        true
    }

    fn on_clip(self: &Arc<Self>, peer: &str, asm: &mut HashMap<[u8; 16], Asm>, meta: ClipMeta) -> bool {
        let s = self.settings.read().unwrap().clone();
        let cap = (s.max_blob_mb.max(1) as usize) * 1024 * 1024;
        if meta.id.len() != 32 {
            return false;
        }
        match meta.kind.as_str() {
            "text" => {
                let Some(t) = meta.text.clone() else { return true };
                if t.len() > MAX_TEXT || content_id("text", "", t.as_bytes()) != meta.id {
                    return true; // id não bate com o conteúdo: descarta sem derrubar a conexão
                }
                let item = NewItem { kind: "text".into(), name: String::new(), mime: "text/plain".into(), text: Some(t.clone()), origin: peer.into() };
                self.stored(&meta, self.store.upsert(&meta.id, &item, t.len() as i64, meta.ts, None), Payload::Text(t), &s);
            }
            "image" | "file" => {
                let ok_kind = if meta.kind == "image" { s.sync_images } else { s.sync_files };
                let needed = self.store.has_row(&meta.id).map_or(true, |mine| meta.ts > mine);
                if !ok_kind || !needed || meta.size <= 0 || meta.size as usize > cap {
                    return true;
                }
                if let Ok(id) = unhex_n::<16>(&meta.id) {
                    if asm.len() < 4 {
                        asm.insert(id, Asm { meta: meta.clone(), buf: Vec::with_capacity(meta.size as usize) });
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn on_chunk(self: &Arc<Self>, peer: &str, asm: &mut HashMap<[u8; 16], Asm>, plain: &[u8]) -> bool {
        let id: [u8; 16] = plain[1..17].try_into().unwrap();
        let off = u32::from_be_bytes(plain[17..21].try_into().unwrap()) as usize;
        let data = &plain[21..];
        let Some(a) = asm.get_mut(&id) else { return true };
        if off != a.buf.len() || a.buf.len() + data.len() > a.meta.size as usize {
            asm.remove(&id);
            return true;
        }
        a.buf.extend_from_slice(data);
        if a.buf.len() < a.meta.size as usize {
            return true;
        }
        let a = asm.remove(&id).unwrap();
        let meta = a.meta;
        if content_id(&meta.kind, &meta.name, &a.buf) != meta.id {
            return true; // corrompido ou forjado
        }
        let s = self.settings.read().unwrap().clone();
        let item = NewItem { kind: meta.kind.clone(), name: meta.name.clone(), mime: meta.mime.clone(), text: None, origin: peer.into() };
        let payload = if meta.kind == "image" { Payload::Image { png: a.buf.clone() } } else { Payload::File { name: meta.name.clone(), mime: meta.mime.clone(), bytes: a.buf.clone() } };
        let r = self.store.upsert(&meta.id, &item, a.buf.len() as i64, meta.ts, Some(&a.buf));
        self.stored(&meta, r, payload, &s);
        true
    }

    fn stored(&self, meta: &ClipMeta, r: Result<Upsert>, payload: Payload, s: &Settings) {
        let Ok(r) = r else { return };
        if r == Upsert::Unchanged {
            return;
        }
        (self.events)(Event::History);
        let _ = self.purge();
        if meta.live && !s.paused {
            *self.last_applied.lock().unwrap() = Some((meta.id.clone(), Instant::now()));
            (self.events)(Event::Apply { id: meta.id.clone(), payload });
        }
    }

    fn send_item(&self, tx: &Sender<Out>, id: &str, live: bool) {
        let Ok(Some(it)) = self.store.get(id) else { return };
        let text = if it.kind == "text" { self.store.full_text(id) } else { None };
        if it.kind == "text" && text.is_none() {
            return;
        }
        let meta = ClipMeta { id: id.into(), ts: it.ts, kind: it.kind.clone(), mime: it.mime, name: it.name, size: it.size, live, text };
        if it.kind != "text" {
            let Some(b) = self.store.blob(id) else { return };
            let Ok(raw) = unhex_n::<16>(id) else { return };
            let _ = tx.send(Out::Msg(Msg::Clip(ClipMeta { size: b.len() as i64, ..meta })));
            for (i, ch) in b.chunks(CHUNK).enumerate() {
                let _ = tx.send(Out::Chunk { id: raw, off: (i * CHUNK) as u32, data: ch.to_vec() });
            }
        } else {
            let _ = tx.send(Out::Msg(Msg::Clip(meta)));
        }
    }

    fn broadcast_item(&self, id: &str, live: bool) {
        let txs: Vec<Sender<Out>> = self.conns.lock().unwrap().values().map(|c| c.tx.clone()).collect();
        for tx in txs {
            self.send_item(&tx, id, live);
        }
    }
}
