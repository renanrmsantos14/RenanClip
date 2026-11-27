//! Pareamento com código de 6 dígitos mostrado nos DOIS aparelhos (comparação numérica
//! com compromisso, como o Bluetooth LE Secure Connections). Nada de segredo digitado:
//! um intermediário só passa com chance de 1 em 1.000.000 por tentativa.
//!
//! 1. I→R `p1`: id, nome, commit = SHA256("RCLP-commit" || I_pub || nI)
//! 2. R→I `p2`: id, nome, R_pub, nR
//! 3. I→R `p3`: I_pub, nI (R confere o compromisso)
//! 4. ambos calculam transcrição, código e chave do par; o usuário confere e aceita
//! 5. `p4`: HMAC(chave, "RCLP-confirm" || papel || transcrição) em cada sentido

use crate::crypto::*;
use crate::frame::*;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t")]
enum PairMsg {
    #[serde(rename = "p1")]
    P1 { id: String, name: String, commit: String },
    #[serde(rename = "p2")]
    P2 { id: String, name: String, pb: String, n: String },
    #[serde(rename = "p3")]
    P3 { pb: String, n: String },
    #[serde(rename = "p4")]
    P4 { mac: String },
    #[serde(rename = "p5")]
    P5 {},
}

const MAX_PAIR_FRAME: usize = 4096;

#[derive(Debug, Clone)]
pub struct Paired {
    pub peer_id: [u8; 16],
    pub peer_name: String,
    pub key: [u8; 32],
}

pub struct Transcript {
    pub i_id: [u8; 16],
    pub r_id: [u8; 16],
    pub i_pub: [u8; 65],
    pub r_pub: [u8; 65],
    pub n_i: [u8; 16],
    pub n_r: [u8; 16],
}

impl Transcript {
    pub fn hash(&self) -> [u8; 32] {
        sha256(&[b"RCLP-pair", &self.i_id, &self.r_id, &self.i_pub, &self.r_pub, &self.n_i, &self.n_r])
    }
    /// Código de 6 dígitos exibido aos dois lados.
    pub fn sas(&self) -> String {
        let h = sha256(&[b"RCLP-sas", &self.hash()]);
        let v = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000;
        format!("{v:06}")
    }
    pub fn pair_key(&self, shared: &[u8; 32]) -> [u8; 32] {
        let mut k = [0u8; 32];
        hkdf(&self.hash(), shared, b"RCLP pair key", &mut k);
        k
    }
    pub fn confirm_mac(&self, key: &[u8; 32], role: u8) -> [u8; 32] {
        hmac(key, &[b"RCLP-confirm", &[role], &self.hash()])
    }
}

pub fn commit(i_pub: &[u8; 65], n_i: &[u8; 16]) -> [u8; 32] {
    sha256(&[b"RCLP-commit", i_pub, n_i])
}

fn send<S: Write>(s: &mut S, m: &PairMsg) -> Result<()> {
    write_frame(s, &serde_json::to_vec(m)?)
}

fn recv<S: Read>(s: &mut S) -> Result<PairMsg> {
    Ok(serde_json::from_slice(&read_frame(s, MAX_PAIR_FRAME)?)?)
}

/// `decide(peer_id_hex, peer_name, sas)` bloqueia até o usuário aceitar ou recusar.
pub type Decide<'a> = &'a mut dyn FnMut(&str, &str, &str) -> bool;

pub fn initiate<S: Read + Write>(s: &mut S, my_id: &[u8; 16], my_name: &str, decide: Decide) -> Result<Paired> {
    let eph = Ephemeral::new();
    let i_pub = eph.public();
    let n_i = rand_bytes::<16>();
    send(s, &PairMsg::P1 { id: hex(my_id), name: my_name.into(), commit: hex(&commit(&i_pub, &n_i)) })?;
    let (r_id_hex, r_name, r_pub_hex, n_r_hex) = match recv(s)? {
        PairMsg::P2 { id, name, pb, n } => (id, name, pb, n),
        PairMsg::P5 {} => return Err(Error::Proto("recusado pelo outro aparelho".into())),
        _ => return Err(Error::Proto("esperava p2".into())),
    };
    let r_id: [u8; 16] = unhex_n(&r_id_hex)?;
    let r_pub: [u8; 65] = unhex_n(&r_pub_hex)?;
    let n_r: [u8; 16] = unhex_n(&n_r_hex)?;
    send(s, &PairMsg::P3 { pb: hex(&i_pub), n: hex(&n_i) })?;
    let shared = eph.agree(&r_pub)?;
    let t = Transcript { i_id: *my_id, r_id, i_pub, r_pub, n_i, n_r };
    finish(s, &t, &shared, b'I', r_id, r_name, decide)
}

pub fn respond<S: Read + Write>(s: &mut S, my_id: &[u8; 16], my_name: &str, decide: Decide) -> Result<Paired> {
    let (i_id_hex, i_name, commit_hex) = match recv(s)? {
        PairMsg::P1 { id, name, commit } => (id, name, commit),
        _ => return Err(Error::Proto("esperava p1".into())),
    };
    let i_id: [u8; 16] = unhex_n(&i_id_hex)?;
    let want_commit: [u8; 32] = unhex_n(&commit_hex)?;
    let eph = Ephemeral::new();
    let r_pub = eph.public();
    let n_r = rand_bytes::<16>();
    send(s, &PairMsg::P2 { id: hex(my_id), name: my_name.into(), pb: hex(&r_pub), n: hex(&n_r) })?;
    let (i_pub_hex, n_i_hex) = match recv(s)? {
        PairMsg::P3 { pb, n } => (pb, n),
        _ => return Err(Error::Proto("esperava p3".into())),
    };
    let i_pub: [u8; 65] = unhex_n(&i_pub_hex)?;
    let n_i: [u8; 16] = unhex_n(&n_i_hex)?;
    if commit(&i_pub, &n_i) != want_commit {
        return Err(Error::Proto("compromisso não confere".into()));
    }
    let shared = eph.agree(&i_pub)?;
    let t = Transcript { i_id, r_id: *my_id, i_pub, r_pub, n_i, n_r };
    finish(s, &t, &shared, b'R', i_id, i_name, decide)
}

fn finish<S: Read + Write>(
    s: &mut S,
    t: &Transcript,
    shared: &[u8; 32],
    role: u8,
    peer_id: [u8; 16],
    peer_name: String,
    decide: Decide,
) -> Result<Paired> {
    let key = t.pair_key(shared);
    let sas = t.sas();
    if !decide(&hex(&peer_id), &peer_name, &sas) {
        let _ = send(s, &PairMsg::P5 {});
        return Err(Error::Proto("pareamento recusado".into()));
    }
    send(s, &PairMsg::P4 { mac: hex(&t.confirm_mac(&key, role)) })?;
    let peer_role = if role == b'I' { b'R' } else { b'I' };
    match recv(s)? {
        PairMsg::P4 { mac } => {
            let mac = unhex(&mac).ok_or(Error::Proto("mac".into()))?;
            if !hmac_verify(&key, &[b"RCLP-confirm", &[peer_role], &t.hash()], &mac) {
                return Err(Error::Proto("confirmação inválida (chaves diferentes)".into()));
            }
        }
        PairMsg::P5 {} => return Err(Error::Proto("recusado pelo outro aparelho".into())),
        _ => return Err(Error::Proto("esperava p4".into())),
    }
    Ok(Paired { peer_id, peer_name, key })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn run(accept_i: bool, accept_r: bool) -> (Result<Paired>, Result<Paired>, String, String) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let r = thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut sas = String::new();
            let res = respond(&mut s, &[2u8; 16], "Celular", &mut |_, _, c| {
                sas = c.to_string();
                accept_r
            });
            (res, sas)
        });
        let mut s = TcpStream::connect(addr).unwrap();
        let mut sas_i = String::new();
        let res_i = initiate(&mut s, &[1u8; 16], "PC", &mut |_, _, c| {
            sas_i = c.to_string();
            accept_i
        });
        let (res_r, sas_r) = r.join().unwrap();
        (res_i, res_r, sas_i, sas_r)
    }

    #[test]
    fn both_sides_get_same_code_and_key() {
        let (i, r, sas_i, sas_r) = run(true, true);
        let (i, r) = (i.unwrap(), r.unwrap());
        assert_eq!(sas_i, sas_r);
        assert_eq!(sas_i.len(), 6);
        assert_eq!(i.key, r.key);
        assert_eq!(i.peer_id, [2u8; 16]);
        assert_eq!(r.peer_id, [1u8; 16]);
        assert_eq!(i.peer_name, "Celular");
        assert_eq!(r.peer_name, "PC");
    }

    #[test]
    fn rejection_by_either_side_fails_both() {
        let (i, r, _, _) = run(true, false);
        assert!(i.is_err() && r.is_err());
        let (i, r, _, _) = run(false, true);
        assert!(i.is_err() && r.is_err());
    }

    #[test]
    fn each_pairing_has_fresh_key_and_code() {
        let (a, ..) = run(true, true);
        let (b, ..) = run(true, true);
        assert_ne!(a.unwrap().key, b.unwrap().key);
    }

    #[test]
    fn man_in_the_middle_sees_different_codes() {
        // M conversa com I fingindo ser R, e com R fingindo ser I: cada lado tem uma
        // transcrição diferente, então os códigos exibidos divergem (chance 1e-6 de colidir).
        let i_eph = Ephemeral::new();
        let r_eph = Ephemeral::new();
        let m1 = Ephemeral::new(); // M diante de I
        let m2 = Ephemeral::new(); // M diante de R
        let (n_i, n_r, n_m1, n_m2) = (rand_bytes::<16>(), rand_bytes::<16>(), rand_bytes::<16>(), rand_bytes::<16>());
        let t_i = Transcript { i_id: [1; 16], r_id: [2; 16], i_pub: i_eph.public(), r_pub: m1.public(), n_i, n_r: n_m1 };
        let t_r = Transcript { i_id: [1; 16], r_id: [2; 16], i_pub: m2.public(), r_pub: r_eph.public(), n_i: n_m2, n_r };
        assert_ne!(t_i.sas(), t_r.sas());
        // e as chaves também divergem: M não consegue relayar a confirmação p4
        let k_i = t_i.pair_key(&i_eph.agree(&m1.public()).unwrap());
        let k_r = t_r.pair_key(&r_eph.agree(&m2.public()).unwrap());
        assert_ne!(k_i, k_r);
        assert!(!hmac_verify(&k_r, &[b"RCLP-confirm", &[b'I'], &t_r.hash()], &t_i.confirm_mac(&k_i, b'I')));
    }

    #[test]
    fn tampered_commit_is_rejected() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let r = thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            respond(&mut s, &[2u8; 16], "R", &mut |_, _, _| true)
        });
        // iniciador malicioso: revela chave diferente da comprometida
        let mut s = TcpStream::connect(addr).unwrap();
        let e1 = Ephemeral::new();
        let e2 = Ephemeral::new();
        let n = rand_bytes::<16>();
        send(&mut s, &PairMsg::P1 { id: hex(&[1u8; 16]), name: "X".into(), commit: hex(&commit(&e1.public(), &n)) }).unwrap();
        let _ = recv(&mut s).unwrap();
        send(&mut s, &PairMsg::P3 { pb: hex(&e2.public()), n: hex(&n) }).unwrap();
        assert!(r.join().unwrap().is_err());
    }
}
