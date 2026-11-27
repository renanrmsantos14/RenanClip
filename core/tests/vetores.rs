//! Vetores determinísticos para a implementação Kotlin. `RENANCLIP_GEN_VECTORS=1` regrava o arquivo.
use renanclip_core::crypto::*;
use renanclip_core::pairing::{commit, Transcript};
use renanclip_core::store::content_id;
use serde_json::json;

fn gen() -> serde_json::Value {
    let a = Ephemeral::from_scalar(&[0x11; 32]).unwrap();
    let b = Ephemeral::from_scalar(&[0x22; 32]).unwrap();
    let shared = a.agree(&b.public()).unwrap();
    assert_eq!(shared, b.agree(&a.public()).unwrap());
    let t = Transcript { i_id: [1; 16], r_id: [2; 16], i_pub: a.public(), r_pub: b.public(), n_i: [3; 16], n_r: [4; 16] };
    let key = t.pair_key(&shared);
    let (c2s, s2c) = session_keys(&key, &[5; 16], &[6; 16]);
    let mut sc = Seal::new(&c2s, Dir::ClientToServer);
    let mut ss = Seal::new(&s2c, Dir::ServerToClient);
    json!({
        "ecdh": {"a_scalar": hex(&[0x11; 32]), "b_scalar": hex(&[0x22; 32]), "a_pub": hex(&a.public()), "b_pub": hex(&b.public()), "shared": hex(&shared)},
        "pairing": {
            "i_id": hex(&[1; 16]), "r_id": hex(&[2; 16]), "n_i": hex(&[3; 16]), "n_r": hex(&[4; 16]),
            "commit": hex(&commit(&a.public(), &[3; 16])), "transcript_hash": hex(&t.hash()), "sas": t.sas(),
            "pair_key": hex(&key), "confirm_i": hex(&t.confirm_mac(&key, b'I')), "confirm_r": hex(&t.confirm_mac(&key, b'R')),
        },
        "session": {
            "n_client": hex(&[5; 16]), "n_server": hex(&[6; 16]), "c2s": hex(&c2s), "s2c": hex(&s2c),
            "c2s_frames": [hex(&sc.seal(b"primeiro").unwrap()), hex(&sc.seal(b"segundo").unwrap())],
            "s2c_frames": [hex(&ss.seal(b"resposta").unwrap())],
            "plain_c2s": ["primeiro", "segundo"], "plain_s2c": ["resposta"],
        },
        "content_id": {
            "text_ola": content_id("text", "", "olá".as_bytes()),
            "file_a_txt": content_id("file", "a.txt", b"abc"),
            "image_png": content_id("image", "", &[1, 2, 3]),
        },
    })
}

#[test]
fn vetores_batem_com_o_arquivo() {
    let atual = serde_json::to_string_pretty(&gen()).unwrap() + "\n";
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/vetores.json");
    if std::env::var("RENANCLIP_GEN_VECTORS").is_ok() {
        std::fs::write(path, &atual).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).unwrap_or_default(), atual, "rode com RENANCLIP_GEN_VECTORS=1 para regravar");
}
