//! Primitivas: ECDH P-256, SHA-256, HKDF, HMAC e AES-256-GCM em fluxo.
//! Tudo aqui tem equivalente direto no JCA do Android (veja docs/protocolo.md).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use p256::ecdh::diffie_hellman;
use p256::{EncodedPoint, PublicKey, SecretKey};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Crypto(&'static str),
    Proto(String),
    Io(std::io::Error),
    Db(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Crypto(s) => write!(f, "cripto: {s}"),
            Error::Proto(s) => write!(f, "protocolo: {s}"),
            Error::Io(e) => write!(f, "e/s: {e}"),
            Error::Db(s) => write!(f, "banco: {s}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Proto(e.to_string())
    }
}

pub fn rand_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

pub fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let v = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    s.as_bytes()
        .chunks(2)
        .map(|p| Some(v(p[0])? << 4 | v(p[1])?))
        .collect()
}

pub fn unhex_n<const N: usize>(s: &str) -> Result<[u8; N]> {
    let v = unhex(s).ok_or(Error::Proto("hex inválido".into()))?;
    v.try_into().map_err(|_| Error::Proto("tamanho inválido".into()))
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], out: &mut [u8]) {
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info, out)
        .expect("saída HKDF válida");
}

pub fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("chave HMAC");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

pub fn hmac_verify(key: &[u8], parts: &[&[u8]], tag: &[u8]) -> bool {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("chave HMAC");
    for p in parts {
        m.update(p);
    }
    m.verify_slice(tag).is_ok()
}

/// Par ECDH P-256 efêmero. Chave pública no formato SEC1 não comprimido (65 bytes).
pub struct Ephemeral {
    secret: SecretKey,
}

impl Ephemeral {
    pub fn new() -> Self {
        Self { secret: SecretKey::random(&mut OsRng) }
    }
    pub fn from_scalar(b: &[u8; 32]) -> Result<Self> {
        Ok(Self { secret: SecretKey::from_slice(b).map_err(|_| Error::Crypto("escalar"))? })
    }
    pub fn public(&self) -> [u8; 65] {
        let p = EncodedPoint::from(self.secret.public_key());
        p.as_bytes().try_into().expect("65 bytes")
    }
    /// Devolve a coordenada x do segredo compartilhado (32 bytes).
    pub fn agree(&self, peer_pub: &[u8]) -> Result<[u8; 32]> {
        if peer_pub.len() != 65 {
            return Err(Error::Crypto("chave pública"));
        }
        let pk = PublicKey::from_sec1_bytes(peer_pub).map_err(|_| Error::Crypto("ponto inválido"))?;
        let shared = diffie_hellman(self.secret.to_nonzero_scalar(), pk.as_affine());
        let mut out = [0u8; 32];
        out.copy_from_slice(shared.raw_secret_bytes());
        Ok(out)
    }
}

impl Default for Ephemeral {
    fn default() -> Self {
        Self::new()
    }
}

/// Direção do fluxo: o cliente é quem abriu a conexão TCP.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    ClientToServer = 1,
    ServerToClient = 2,
}

/// Uma direção cifrada: nonce = 4 zeros || contador u64 BE; AAD = "RCLP" || dir.
pub struct Seal {
    cipher: Aes256Gcm,
    dir: Dir,
    ctr: u64,
}

impl Seal {
    pub fn new(key: &[u8; 32], dir: Dir) -> Self {
        Self { cipher: Aes256Gcm::new(key.into()), dir, ctr: 0 }
    }
    fn nonce(&self) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&self.ctr.to_be_bytes());
        n
    }
    fn aad(&self) -> [u8; 5] {
        [b'R', b'C', b'L', b'P', self.dir as u8]
    }
    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>> {
        let n = self.nonce();
        let out = self
            .cipher
            .encrypt(Nonce::from_slice(&n), Payload { msg: plain, aad: &self.aad() })
            .map_err(|_| Error::Crypto("cifrar"))?;
        self.ctr += 1;
        Ok(out)
    }
    pub fn open(&mut self, ct: &[u8]) -> Result<Vec<u8>> {
        let n = self.nonce();
        let out = self
            .cipher
            .decrypt(Nonce::from_slice(&n), Payload { msg: ct, aad: &self.aad() })
            .map_err(|_| Error::Crypto("decifrar"))?;
        self.ctr += 1;
        Ok(out)
    }
}

/// Chaves de sessão a partir da chave do par e dos dois nonces (cliente primeiro).
pub fn session_keys(pair_key: &[u8; 32], n_client: &[u8; 16], n_server: &[u8; 16]) -> ([u8; 32], [u8; 32]) {
    let mut salt = [0u8; 32];
    salt[..16].copy_from_slice(n_client);
    salt[16..].copy_from_slice(n_server);
    let mut okm = [0u8; 64];
    hkdf(&salt, pair_key, b"RCLP session v1", &mut okm);
    let mut c2s = [0u8; 32];
    let mut s2c = [0u8; 32];
    c2s.copy_from_slice(&okm[..32]);
    s2c.copy_from_slice(&okm[32..]);
    (c2s, s2c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let b = [0u8, 1, 0xab, 0xff];
        assert_eq!(unhex(&hex(&b)).unwrap(), b);
        assert!(unhex("zz").is_none());
        assert!(unhex("abc").is_none());
    }

    #[test]
    fn ecdh_both_sides_agree() {
        let a = Ephemeral::new();
        let b = Ephemeral::new();
        assert_eq!(a.agree(&b.public()).unwrap(), b.agree(&a.public()).unwrap());
    }

    #[test]
    fn ecdh_rejects_garbage() {
        let a = Ephemeral::new();
        assert!(a.agree(&[4u8; 65]).is_err());
        assert!(a.agree(&[0u8; 10]).is_err());
    }

    #[test]
    fn seal_roundtrip_and_order() {
        let k = [7u8; 32];
        let mut tx = Seal::new(&k, Dir::ClientToServer);
        let mut rx = Seal::new(&k, Dir::ClientToServer);
        let c1 = tx.seal(b"um").unwrap();
        let c2 = tx.seal(b"dois").unwrap();
        // fora de ordem não decifra (contador implícito)
        assert!(rx.open(&c2).is_err());
        assert_eq!(rx.open(&c1).unwrap(), b"um");
        assert_eq!(rx.open(&c2).unwrap(), b"dois");
        // repetição não decifra
        assert!(rx.open(&c1).is_err());
    }

    #[test]
    fn seal_direction_is_bound() {
        let k = [7u8; 32];
        let mut tx = Seal::new(&k, Dir::ClientToServer);
        let mut rx = Seal::new(&k, Dir::ServerToClient);
        let c = tx.seal(b"x").unwrap();
        assert!(rx.open(&c).is_err(), "reflexão deve falhar");
    }

    #[test]
    fn wrong_key_does_not_decrypt() {
        let mut tx = Seal::new(&[1u8; 32], Dir::ClientToServer);
        let mut rx = Seal::new(&[2u8; 32], Dir::ClientToServer);
        let c = tx.seal(b"segredo").unwrap();
        assert!(rx.open(&c).is_err());
    }

    #[test]
    fn tampering_is_detected() {
        let k = [3u8; 32];
        let mut tx = Seal::new(&k, Dir::ServerToClient);
        let mut rx = Seal::new(&k, Dir::ServerToClient);
        let mut c = tx.seal(b"integridade").unwrap();
        c[0] ^= 1;
        assert!(rx.open(&c).is_err());
    }
}
