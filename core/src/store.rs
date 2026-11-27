//! Histórico local em SQLite. O id de um item é o hash do conteúdo (texto, imagem ou
//! arquivo): o mesmo conteúdo copiado em dois aparelhos é o MESMO item, então a
//! deduplicação e a sincronização não precisam de alias nem de relógio sincronizado.

use crate::crypto::{hex, sha256, Error, Result};
use rusqlite::{functions::FunctionFlags, params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

pub const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub kind: String, // text | image | file
    pub name: String,
    pub mime: String,
    pub size: i64,
    /// Texto completo (kind=text) cortado em `PREVIEW_CHARS` na listagem.
    pub text: Option<String>,
    pub ts: i64,
    pub pinned: bool,
    pub origin: String,
}

pub const PREVIEW_CHARS: usize = 2000;

#[derive(Clone, Debug)]
pub struct NewItem {
    pub kind: String,
    pub name: String,
    pub mime: String,
    pub text: Option<String>,
    pub origin: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub alias: String,
    pub addr: String,
    pub paired_at: i64,
    pub last_seen: i64,
}

impl Peer {
    pub fn display(&self) -> &str {
        if self.alias.is_empty() { &self.name } else { &self.alias }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Upsert {
    Inserted,
    Bumped,
    Unchanged,
}

/// Id = SHA-256(kind || 0 || name || 0 || conteúdo), primeiros 16 bytes em hex.
pub fn content_id(kind: &str, name: &str, payload: &[u8]) -> String {
    hex(&sha256(&[kind.as_bytes(), &[0], name.as_bytes(), &[0], payload])[..16])
}

/// Minúsculas e sem acento (pt-BR), para a busca.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

pub struct Store {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS items(
  id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL DEFAULT '', mime TEXT NOT NULL DEFAULT '',
  size INTEGER NOT NULL DEFAULT 0, text TEXT, ts INTEGER NOT NULL, pinned INTEGER NOT NULL DEFAULT 0,
  deleted INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER NOT NULL DEFAULT 0, origin TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS items_ts ON items(deleted, pinned, ts);
CREATE TABLE IF NOT EXISTS blobs(id TEXT PRIMARY KEY, data BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS peers(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, alias TEXT NOT NULL DEFAULT '', key BLOB NOT NULL,
  addr TEXT NOT NULL DEFAULT '', paired_at INTEGER NOT NULL, last_seen INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS settings(k TEXT PRIMARY KEY, v TEXT NOT NULL);
";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Self::init(conn)
    }

    pub fn open_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA secure_delete=ON;
             PRAGMA temp_store=MEMORY; PRAGMA cache_size=-2048;",
        )?;
        conn.create_scalar_function("fold", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
            Ok(fold(&ctx.get::<String>(0).unwrap_or_default()))
        })?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---------- configurações ----------
    pub fn setting(&self, k: &str) -> Option<String> {
        self.c().query_row("SELECT v FROM settings WHERE k=?", [k], |r| r.get(0)).optional().ok().flatten()
    }
    pub fn set_setting(&self, k: &str, v: &str) -> Result<()> {
        self.c().execute("INSERT INTO settings(k,v) VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [k, v])?;
        Ok(())
    }

    // ---------- itens ----------
    pub fn upsert(&self, id: &str, n: &NewItem, size: i64, ts: i64, blob: Option<&[u8]>) -> Result<Upsert> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let cur: Option<(i64, i64, i64)> = tx
            .query_row("SELECT ts, deleted, deleted_at FROM items WHERE id=?", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        let res = match cur {
            None => {
                tx.execute(
                    "INSERT INTO items(id,kind,name,mime,size,text,ts,origin) VALUES(?,?,?,?,?,?,?,?)",
                    params![id, n.kind, n.name, n.mime, size, n.text, ts, n.origin],
                )?;
                Upsert::Inserted
            }
            Some((_, 1, deleted_at)) if ts > deleted_at => {
                tx.execute(
                    "UPDATE items SET kind=?,name=?,mime=?,size=?,text=?,ts=?,origin=?,deleted=0,deleted_at=0 WHERE id=?",
                    params![n.kind, n.name, n.mime, size, n.text, ts, n.origin, id],
                )?;
                Upsert::Inserted
            }
            Some((_, 1, _)) => Upsert::Unchanged,
            Some((old, _, _)) if ts > old => {
                tx.execute("UPDATE items SET ts=?, origin=? WHERE id=?", params![ts, n.origin, id])?;
                Upsert::Bumped
            }
            Some(_) => Upsert::Unchanged,
        };
        if res == Upsert::Inserted {
            if let Some(b) = blob {
                tx.execute("INSERT OR REPLACE INTO blobs(id,data) VALUES(?,?)", params![id, b])?;
            }
        }
        tx.commit()?;
        Ok(res)
    }

    pub fn exists_live(&self, id: &str) -> bool {
        self.c().query_row("SELECT 1 FROM items WHERE id=? AND deleted=0", [id], |_| Ok(())).optional().ok().flatten().is_some()
    }

    pub fn has_row(&self, id: &str) -> Option<i64> {
        self.c().query_row("SELECT max(ts, deleted_at) FROM items WHERE id=?", [id], |r| r.get(0)).optional().ok().flatten()
    }

    fn row(r: &rusqlite::Row) -> rusqlite::Result<Item> {
        Ok(Item {
            id: r.get(0)?,
            kind: r.get(1)?,
            name: r.get(2)?,
            mime: r.get(3)?,
            size: r.get(4)?,
            text: r.get(5)?,
            ts: r.get(6)?,
            pinned: r.get::<_, i64>(7)? != 0,
            origin: r.get(8)?,
        })
    }

    const COLS: &'static str = "id,kind,name,mime,size,substr(text,1,2000),ts,pinned,origin";

    /// Fixados primeiro, depois do mais novo para o mais antigo. `query` busca sem acento.
    pub fn history(&self, query: &str, kind: Option<&str>, limit: usize) -> Result<Vec<Item>> {
        let c = self.c();
        let q = fold(query.trim());
        let sql = format!(
            "SELECT {} FROM items WHERE deleted=0 AND (?1='' OR instr(fold(coalesce(text,'')||' '||name),?1)>0)
             AND (?2='' OR kind=?2) ORDER BY pinned DESC, ts DESC LIMIT ?3",
            Self::COLS
        );
        let mut st = c.prepare_cached(&sql)?;
        let rows = st.query_map(params![q, kind.unwrap_or(""), limit as i64], Self::row)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn get(&self, id: &str) -> Result<Option<Item>> {
        let sql = format!("SELECT {} FROM items WHERE id=? AND deleted=0", Self::COLS);
        Ok(self.c().query_row(&sql, [id], Self::row).optional()?)
    }

    pub fn full_text(&self, id: &str) -> Option<String> {
        self.c().query_row("SELECT text FROM items WHERE id=? AND deleted=0", [id], |r| r.get(0)).optional().ok().flatten().flatten()
    }

    pub fn blob(&self, id: &str) -> Option<Vec<u8>> {
        self.c().query_row("SELECT data FROM blobs WHERE id=?", [id], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        self.c().execute("UPDATE items SET pinned=? WHERE id=? AND deleted=0", params![pinned as i64, id])?;
        Ok(())
    }

    /// Apagar de verdade: some o conteúdo e fica só a lápide (id e data) para a
    /// sincronização não ressuscitar o item a partir de outro aparelho.
    pub fn delete(&self, id: &str, now: i64) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE items SET deleted=1, deleted_at=?, text=NULL, pinned=0, name='', mime='' WHERE id=?",
            params![now, id],
        )?;
        tx.execute("DELETE FROM blobs WHERE id=?", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Apaga tudo que não está fixado.
    pub fn clear_unpinned(&self, now: i64) -> Result<usize> {
        let ids: Vec<String> = {
            let c = self.c();
            let mut st = c.prepare("SELECT id FROM items WHERE deleted=0 AND pinned=0")?;
            let rows = st.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
            rows
        };
        for id in &ids {
            self.delete(id, now)?;
        }
        Ok(ids.len())
    }

    /// Retenção: remove (não fixados) o que passou de `days` dias; lápides velhas somem
    /// junto. Depois aplica o teto de tamanho (`max_bytes`, 0 = sem teto) apagando os
    /// mais antigos não fixados.
    pub fn purge(&self, now: i64, days: i64, max_bytes: i64) -> Result<usize> {
        let cutoff = now - days.max(1) * DAY_MS;
        let c = self.c();
        let mut n = c.execute("DELETE FROM items WHERE pinned=0 AND deleted=0 AND ts<?", [cutoff])?;
        n += c.execute("DELETE FROM items WHERE deleted=1 AND deleted_at<?", [cutoff])?;
        c.execute("DELETE FROM blobs WHERE id NOT IN (SELECT id FROM items WHERE deleted=0)", [])?;
        if max_bytes > 0 {
            loop {
                let total: i64 = c.query_row("SELECT coalesce(sum(length(data)),0) FROM blobs", [], |r| r.get(0))?;
                if total <= max_bytes {
                    break;
                }
                let victim: Option<String> = c
                    .query_row(
                        "SELECT i.id FROM items i JOIN blobs b ON b.id=i.id WHERE i.pinned=0 ORDER BY i.ts ASC LIMIT 1",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?;
                let Some(v) = victim else { break };
                c.execute("DELETE FROM blobs WHERE id=?", [&v])?;
                c.execute("DELETE FROM items WHERE id=?", [&v])?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// (id, ts) dos itens vivos mais novos, para o inventário trocado ao conectar.
    pub fn inventory(&self, limit: usize) -> Result<Vec<(String, i64)>> {
        let c = self.c();
        let mut st = c.prepare("SELECT id, ts FROM items WHERE deleted=0 ORDER BY ts DESC LIMIT ?")?;
        let rows = st.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn count(&self) -> i64 {
        self.c().query_row("SELECT count(*) FROM items WHERE deleted=0", [], |r| r.get(0)).unwrap_or(0)
    }

    /// Item mais recente (para a guarda contra repetição do mesmo conteúdo).
    pub fn latest_id(&self) -> Option<String> {
        self.c().query_row("SELECT id FROM items WHERE deleted=0 ORDER BY ts DESC LIMIT 1", [], |r| r.get(0)).optional().ok().flatten()
    }

    // ---------- aparelhos pareados ----------
    pub fn add_peer(&self, id: &str, name: &str, key: &[u8; 32], addr: &str, now: i64) -> Result<()> {
        self.c().execute(
            "INSERT INTO peers(id,name,key,addr,paired_at) VALUES(?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name,key=excluded.key,addr=excluded.addr,paired_at=excluded.paired_at",
            params![id, name, &key[..], addr, now],
        )?;
        Ok(())
    }

    pub fn peer_key(&self, id: &str) -> Option<[u8; 32]> {
        let v: Vec<u8> = self.c().query_row("SELECT key FROM peers WHERE id=?", [id], |r| r.get(0)).optional().ok().flatten()?;
        v.try_into().ok()
    }

    pub fn peers(&self) -> Vec<Peer> {
        let c = self.c();
        let Ok(mut st) = c.prepare("SELECT id,name,alias,addr,paired_at,last_seen FROM peers ORDER BY paired_at") else { return vec![] };
        let rows = st.query_map([], |r| {
            Ok(Peer { id: r.get(0)?, name: r.get(1)?, alias: r.get(2)?, addr: r.get(3)?, paired_at: r.get(4)?, last_seen: r.get(5)? })
        });
        match rows {
            Ok(r) => r.filter_map(|x| x.ok()).collect(),
            Err(_) => vec![],
        }
    }

    pub fn rename_peer(&self, id: &str, alias: &str) -> Result<()> {
        self.c().execute("UPDATE peers SET alias=? WHERE id=?", params![alias.trim(), id])?;
        Ok(())
    }

    pub fn touch_peer(&self, id: &str, addr: &str, now: i64) -> Result<()> {
        self.c().execute("UPDATE peers SET last_seen=?, addr=CASE WHEN ?<>'' THEN ? ELSE addr END WHERE id=?", params![now, addr, addr, id])?;
        Ok(())
    }

    pub fn remove_peer(&self, id: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM peers WHERE id=?", [id])? > 0)
    }
}

impl From<std::sync::PoisonError<std::sync::MutexGuard<'_, Connection>>> for Error {
    fn from(_: std::sync::PoisonError<std::sync::MutexGuard<'_, Connection>>) -> Self {
        Error::Db("trava envenenada".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> (String, NewItem) {
        (content_id("text", "", t.as_bytes()), NewItem { kind: "text".into(), name: String::new(), mime: "text/plain".into(), text: Some(t.into()), origin: "a".into() })
    }

    fn put(s: &Store, t: &str, ts: i64) -> Upsert {
        let (id, n) = text(t);
        s.upsert(&id, &n, t.len() as i64, ts, None).unwrap()
    }

    #[test]
    fn same_content_is_one_item_and_bumps() {
        let s = Store::open_memory().unwrap();
        assert_eq!(put(&s, "olá", 100), Upsert::Inserted);
        assert_eq!(put(&s, "olá", 100), Upsert::Unchanged); // dedupe: mesma data
        assert_eq!(put(&s, "olá", 50), Upsert::Unchanged); // mais velho não regride
        assert_eq!(put(&s, "olá", 200), Upsert::Bumped);
        assert_eq!(s.count(), 1);
        assert_eq!(s.history("", None, 10).unwrap()[0].ts, 200);
    }

    #[test]
    fn search_ignores_accents_and_case() {
        let s = Store::open_memory().unwrap();
        put(&s, "Reunião de Conciliação", 1);
        put(&s, "outra coisa", 2);
        assert_eq!(s.history("REUNIAO", None, 10).unwrap().len(), 1);
        assert_eq!(s.history("conciliacao", None, 10).unwrap().len(), 1);
        assert_eq!(s.history("", None, 10).unwrap().len(), 2);
        assert_eq!(s.history("zzz", None, 10).unwrap().len(), 0);
    }

    #[test]
    fn pinned_first_then_newest() {
        let s = Store::open_memory().unwrap();
        put(&s, "a", 1);
        put(&s, "b", 2);
        put(&s, "c", 3);
        s.set_pinned(&content_id("text", "", b"a"), true).unwrap();
        let h: Vec<_> = s.history("", None, 10).unwrap().into_iter().map(|i| i.text.unwrap()).collect();
        assert_eq!(h, ["a", "c", "b"]);
    }

    #[test]
    fn retention_removes_old_but_keeps_pinned() {
        let s = Store::open_memory().unwrap();
        let now = 100 * DAY_MS;
        put(&s, "velho", now - 8 * DAY_MS);
        put(&s, "velho fixado", now - 30 * DAY_MS);
        put(&s, "novo", now - DAY_MS);
        s.set_pinned(&content_id("text", "", "velho fixado".as_bytes()), true).unwrap();
        assert_eq!(s.purge(now, 7, 0).unwrap(), 1);
        let h: Vec<_> = s.history("", None, 10).unwrap().into_iter().map(|i| i.text.unwrap()).collect();
        assert_eq!(h, ["velho fixado", "novo"]);
    }

    #[test]
    fn delete_erases_content_and_blocks_stale_resurrection() {
        let s = Store::open_memory().unwrap();
        put(&s, "senha123", 100);
        let id = content_id("text", "", b"senha123");
        s.delete(&id, 500).unwrap();
        assert!(s.get(&id).unwrap().is_none());
        assert!(s.full_text(&id).is_none());
        // outro aparelho oferece a mesma cópia antiga: não volta
        assert_eq!(put(&s, "senha123", 100), Upsert::Unchanged);
        assert_eq!(put(&s, "senha123", 499), Upsert::Unchanged);
        // copiar de novo depois da exclusão volta
        assert_eq!(put(&s, "senha123", 600), Upsert::Inserted);
        assert!(s.get(&id).unwrap().is_some());
    }

    #[test]
    fn blob_roundtrip_and_size_cap() {
        let s = Store::open_memory().unwrap();
        let data = vec![9u8; 1000];
        let id = content_id("image", "", &data);
        let n = NewItem { kind: "image".into(), name: "".into(), mime: "image/png".into(), text: None, origin: "a".into() };
        s.upsert(&id, &n, 1000, 10, Some(&data)).unwrap();
        assert_eq!(s.blob(&id).unwrap(), data);
        let data2 = vec![8u8; 1000];
        let id2 = content_id("image", "", &data2);
        s.upsert(&id2, &n, 1000, 20, Some(&data2)).unwrap();
        // teto de 1500 bytes: o mais antigo sai
        s.purge(30, 7, 1500).unwrap();
        assert!(s.blob(&id).is_none());
        assert!(s.blob(&id2).is_some());
    }

    #[test]
    fn peers_crud() {
        let s = Store::open_memory().unwrap();
        s.add_peer("p1", "Poco", &[5u8; 32], "1.2.3.4:5", 1).unwrap();
        assert_eq!(s.peer_key("p1").unwrap(), [5u8; 32]);
        s.rename_peer("p1", "Meu celular").unwrap();
        assert_eq!(s.peers()[0].display(), "Meu celular");
        assert!(s.remove_peer("p1").unwrap());
        assert!(s.peer_key("p1").is_none());
    }

    #[test]
    fn inventory_lists_live_items_only() {
        let s = Store::open_memory().unwrap();
        put(&s, "a", 1);
        put(&s, "b", 2);
        s.delete(&content_id("text", "", b"a"), 5).unwrap();
        let inv = s.inventory(10).unwrap();
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].1, 2);
    }
}
