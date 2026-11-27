//! Integração: duas Engines em loopback (sem mDNS), perfis separados.
use renanclip_core::*;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Node {
    e: Engine,
    ev: Arc<Mutex<Vec<Event>>>,
    dir: std::path::PathBuf,
}

fn node(tag: &str) -> Node {
    node_with(tag, |_| {})
}
fn node_with(tag: &str, tweak: impl FnOnce(&mut Config)) -> Node {
    let dir = std::env::temp_dir().join(format!("renanclip-test-{}-{}-{}", tag, std::process::id(), rand_tag()));
    let ev = Arc::new(Mutex::new(Vec::new()));
    let ev2 = ev.clone();
    let mut cfg = Config::new(dir.clone());
    cfg.device_name = Some(tag.into());
    cfg.port = 0;
    cfg.mdns = false;
    cfg.tailscale_cmd = Some("renanclip-sem-tailscale".into());
    tweak(&mut cfg);
    let e = Engine::start(cfg, move |x| ev2.lock().unwrap().push(x)).unwrap();
    Node { e, ev, dir }
}
fn rand_tag() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() as u64
}
impl Drop for Node {
    fn drop(&mut self) {
        self.e.shutdown();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn wait(mut f: impl FnMut() -> bool, what: &str) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    panic!("tempo esgotado: {what}");
}
fn addr(n: &Node) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], n.e.port()))
}
/// Pareia a com b aceitando o código nos dois lados; devolve o código visto por cada lado.
fn pair(a: &Node, b: &Node) -> (String, String) {
    b.e.open_pairing(30);
    a.e.pair_with(addr(b));
    let sas = |n: &Node| {
        let mut out = None;
        wait(
            || {
                let g = n.ev.lock().unwrap();
                out = g.iter().find_map(|e| if let Event::PairPrompt { pair_id, sas, .. } = e { Some((*pair_id, sas.clone())) } else { None });
                out.is_some()
            },
            "pedido de pareamento",
        );
        out.unwrap()
    };
    let (pa, sa) = sas(a);
    let (pb, sb) = sas(b);
    a.e.pair_decide(pa, true);
    b.e.pair_decide(pb, true);
    wait(|| a.e.online_count() == 1 && b.e.online_count() == 1, "sessão online");
    (sa, sb)
}
fn texts(n: &Node) -> Vec<String> {
    n.e.history("", None, 100).into_iter().filter_map(|i| i.text).collect()
}

#[test]
fn pareia_e_sincroniza_texto_imagem_arquivo() {
    let (a, b) = (node("a"), node("b"));
    let (sa, sb) = pair(&a, &b);
    assert_eq!(sa, sb, "os dois lados veem o mesmo código");
    assert_eq!(sa.len(), 6);

    a.e.local_copy(LocalCopy::text("olá mundo")).unwrap();
    wait(|| texts(&b).contains(&"olá mundo".to_string()), "texto em b");
    // b recebeu como "aplicar" (live)
    assert!(b.ev.lock().unwrap().iter().any(|e| matches!(e, Event::Apply { payload: Payload::Text(t), .. } if t == "olá mundo")));

    let png: Vec<u8> = (0..600_000u32).map(|i| (i % 251) as u8).collect(); // > 2 chunks
    let id = a.e.local_copy(LocalCopy::image(png.clone())).unwrap();
    wait(|| b.e.payload(&id).is_some(), "imagem em b");
    assert!(matches!(b.e.payload(&id), Some(Payload::Image { png: p }) if p == png));

    let id = b.e.local_copy(LocalCopy::file("nota.txt", "text/plain", b"conteudo".to_vec())).unwrap();
    wait(|| a.e.payload(&id).is_some(), "arquivo em a");
    assert!(matches!(a.e.payload(&id), Some(Payload::File { name, bytes, .. }) if name == "nota.txt" && bytes == b"conteudo"));
}

#[test]
fn dedupe_um_item_so() {
    let (a, b) = (node("a"), node("b"));
    pair(&a, &b);
    a.e.local_copy(LocalCopy::text("repetido")).unwrap();
    wait(|| texts(&b).len() == 1, "chegou em b");
    std::thread::sleep(Duration::from_millis(50));
    a.e.local_copy(LocalCopy::text("repetido"));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(texts(&a).len(), 1);
    assert_eq!(texts(&b).len(), 1);
}

#[test]
fn inventario_traz_o_que_faltava_ao_reconectar() {
    let (a, b) = (node("a"), node("b"));
    pair(&a, &b);
    b.e.shutdown();
    wait(|| a.e.online_count() == 0, "b caiu");
    a.e.local_copy(LocalCopy::text("enquanto b estava fora")).unwrap();
    // b volta no mesmo diretório
    let ev = Arc::new(Mutex::new(Vec::new()));
    let ev2 = ev.clone();
    let mut cfg = Config::new(b.dir.clone());
    cfg.port = 0;
    cfg.mdns = false;
    let b2 = Engine::start(cfg, move |x| ev2.lock().unwrap().push(x)).unwrap();
    // sem mDNS: informa o endereço de a, já pareado
    b2.set_peer_addr(a.e.my_id(), &addr(&a).to_string());
    wait(|| b2.history("", None, 10).iter().any(|i| i.text.as_deref() == Some("enquanto b estava fora")), "inventário sincronizou");
    b2.shutdown();
}

#[test]
fn revogar_derruba_e_apaga_a_chave_nos_dois_lados() {
    let (a, b) = (node("a"), node("b"));
    pair(&a, &b);
    a.e.revoke_peer(b.e.my_id());
    wait(|| a.e.online_count() == 0 && b.e.online_count() == 0, "conexão caiu");
    wait(|| b.e.peers().is_empty(), "b esqueceu a");
    assert!(a.e.peers().is_empty());
    a.e.local_copy(LocalCopy::text("depois da revogação")).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(texts(&b).is_empty(), "nada chega depois de revogar");
}

#[test]
fn par_errado_nao_conecta_nem_decifra() {
    let (a, b, c) = (node("a"), node("b"), node("c"));
    pair(&a, &b);
    // c nunca foi pareado com a: conhece o endereço, mas não tem chave
    c.e.set_peer_addr(a.e.my_id(), &addr(&a).to_string());
    a.e.local_copy(LocalCopy::text("segredo")).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(texts(&c).is_empty());
    assert_eq!(c.e.online_count(), 0);
    wait(|| texts(&b).contains(&"segredo".to_string()), "b recebe");
}

#[test]
fn pareamento_so_com_janela_aberta_e_recusa_falha_os_dois() {
    let (a, b) = (node("a"), node("b"));
    // janela fechada: ninguém vê pedido
    a.e.pair_with(addr(&b));
    std::thread::sleep(Duration::from_millis(500));
    assert!(b.ev.lock().unwrap().iter().all(|e| !matches!(e, Event::PairPrompt { .. })));
    assert!(a.e.peers().is_empty() && b.e.peers().is_empty());
    // janela aberta, b recusa
    b.e.open_pairing(30);
    a.e.pair_with(addr(&b));
    let (mut pa, mut pb) = (0, 0);
    wait(
        || {
            pa = a.ev.lock().unwrap().iter().find_map(|e| if let Event::PairPrompt { pair_id, .. } = e { Some(*pair_id) } else { None }).unwrap_or(0);
            pb = b.ev.lock().unwrap().iter().find_map(|e| if let Event::PairPrompt { pair_id, .. } = e { Some(*pair_id) } else { None }).unwrap_or(0);
            pa != 0 && pb != 0
        },
        "pedidos",
    );
    a.e.pair_decide(pa, true);
    b.e.pair_decide(pb, false);
    wait(|| a.ev.lock().unwrap().iter().any(|e| matches!(e, Event::PairDone { ok: false, .. })), "a falhou");
    assert!(a.e.peers().is_empty() && b.e.peers().is_empty());
}

#[test]
fn sensivel_pausa_e_app_ignorado_nao_entram() {
    let a = node("a");
    let mut c = LocalCopy::text("senha123");
    c.sensitive = true;
    assert!(a.e.local_copy(c).is_none());
    let mut c = LocalCopy::text("do cofre");
    c.source_app = Some("keepassxc".into());
    assert!(a.e.local_copy(c).is_none());
    let mut s = a.e.settings();
    s.paused = true;
    a.e.update_settings(s);
    assert!(a.e.local_copy(LocalCopy::text("pausado")).is_none());
    assert!(texts(&a).is_empty());
}

#[test]
fn retencao_remove_antigos_e_preserva_fixados() {
    let a = node("a");
    let old = a.e.local_copy(LocalCopy::text("velho")).unwrap();
    let pinned = a.e.local_copy(LocalCopy::text("fixo")).unwrap();
    a.e.pin(&pinned, true);
    let day = 86_400_000i64;
    a.e.store().purge(now_ms() + 8 * day, 7, 500 * 1024 * 1024).unwrap();
    let t = texts(&a);
    assert!(!t.contains(&"velho".to_string()) && t.contains(&"fixo".to_string()), "{t:?}");
    assert!(a.e.payload(&old).is_none());
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// Interface simulada: um "tailscale" falso que anuncia 127.0.0.1 como nó online da tailnet.
fn fake_tailscale(dir: &std::path::Path, json: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("tailscale");
    std::fs::write(&p, format!("#!/bin/sh\ncat <<'EOF'\n{json}\nEOF\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn tailnet_acha_o_par_quando_a_lan_falha_e_cifra_continua_propria() {
    let port = free_port();
    let tdir = std::env::temp_dir().join(format!("renanclip-fake-ts-{}-{}", std::process::id(), rand_tag()));
    let cmd = fake_tailscale(&tdir, r#"{"Peer":{"k":{"HostName":"pc-b","DNSName":"pc-b.tail.ts.net.","TailscaleIPs":["127.0.0.1"],"Online":true},"o":{"HostName":"offline","TailscaleIPs":["127.0.0.9"],"Online":false}}}"#);
    let b = node_with("b", |c| c.port = port);
    let a = node_with("a", |c| {
        c.tailscale_cmd = Some(cmd.clone());
        c.tailnet_port = port;
    });
    pair(&a, &b);
    let b_id = b.e.my_id().to_string();
    b.e.shutdown();
    wait(|| a.e.online_count() == 0, "b caiu");
    // endereço guardado ruim (a LAN "falha"); só a tailnet leva até b
    a.e.set_peer_addr(&b_id, "127.0.0.1:1");
    let mut cfg = Config::new(b.dir.clone());
    cfg.port = port;
    cfg.mdns = false;
    let b2 = Engine::start(cfg, |_| {}).unwrap();
    let nodes = a.e.tailnet();
    assert_eq!(nodes.len(), 2);
    assert!(a.e.nearby().iter().any(|n| n.via == "tailnet" && n.addr == format!("127.0.0.1:{port}")), "nó online aparece; o offline não");
    assert!(!a.e.nearby().iter().any(|n| n.addr.starts_with("127.0.0.9")));
    wait(|| a.e.online_count() == 1, "conectou pela tailnet");
    a.e.local_copy(LocalCopy::text("via tailnet")).unwrap();
    wait(|| b2.history("", None, 10).iter().any(|i| i.text.as_deref() == Some("via tailnet")), "texto chegou cifrado");
    b2.shutdown();
    let _ = std::fs::remove_dir_all(tdir);
}

#[test]
fn sem_tailscale_instalado_nao_ha_erro_nem_nos() {
    let a = node("a"); // comando inexistente
    assert!(a.e.tailnet().is_empty());
    assert!(a.e.nearby().is_empty());
}

#[test]
fn resolve_aceita_ip_nome_e_porta() {
    let a = node("a");
    assert_eq!(a.e.resolve("127.0.0.1:5000").unwrap().port(), 5000);
    assert_eq!(a.e.resolve("127.0.0.1").unwrap().port(), 47823);
    assert!(a.e.resolve("localhost").unwrap().ip().is_loopback());
    assert!(a.e.resolve("").is_err());
    assert!(a.e.resolve("host-que-nao-existe.invalid").is_err());
}
