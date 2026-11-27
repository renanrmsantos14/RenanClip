//! RenanClip: núcleo de sincronização da área de transferência (sem interface).
pub mod crypto;
pub mod discovery;
pub mod engine;
pub mod frame;
pub mod pairing;
pub mod proto;
pub mod settings;
pub mod store;

pub use crypto::{Error, Result};
pub use engine::{now_ms, Config, Engine, NearbyView, PeerView};
pub use proto::{Event, LocalCopy, Payload};
pub use settings::Settings;
