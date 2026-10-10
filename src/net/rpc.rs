//! Remote procedure calls on scene objects, sent through a [`NetSession`].
//!
//! A call names an object (by its unique scene name), a method and argument
//! bytes. Register each method once with who may call it and how it
//! travels; the host checks every call a client makes before the game sees
//! it. Clients take any registered call from the host, which is the
//! authority.

use super::{invalid, NetSession, PeerId, HOST};
use std::collections::BTreeMap;
use std::io;

/// Marks an RPC message, so game messages on the same session pass by.
const MAGIC: &[u8; 4] = b"\0rpc";

/// Who may call a method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authority {
    /// Only the host: it runs on clients.
    Host,
    /// The host, or the client that owns the object ([`Rpcs::set_owner`]).
    Owner,
    /// Any peer.
    Anyone,
}

/// How a call travels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reliability {
    /// Arrives once and in order with other reliable messages.
    Reliable,
    /// Over UDP: may be lost or overtaken. For state sent every tick.
    Unreliable,
}

/// A call that passed its checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcCall {
    pub from: PeerId,
    pub object: String,
    pub method: String,
    pub args: Vec<u8>,
}

/// Registered methods and object owners. Keep one per session, with the
/// same methods registered on every peer.
#[derive(Clone, Debug, Default)]
pub struct Rpcs {
    methods: BTreeMap<String, (Authority, Reliability)>,
    owners: BTreeMap<String, PeerId>,
}

impl Rpcs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `method` with who may call it and how it travels.
    pub fn register(
        &mut self,
        method: &str,
        authority: Authority,
        reliability: Reliability,
    ) -> &mut Self {
        self.methods
            .insert(method.to_owned(), (authority, reliability));
        self
    }

    /// Makes `peer` the owner of `object` for [`Authority::Owner`] calls.
    /// The host's record is the one that counts. Objects start owned by
    /// the host.
    pub fn set_owner(&mut self, object: &str, peer: PeerId) {
        self.owners.insert(object.to_owned(), peer);
    }

    pub fn owner(&self, object: &str) -> PeerId {
        self.owners.get(object).copied().unwrap_or(HOST)
    }

    /// Calls `method` on `object`: from a client, on the host; from the
    /// host, on every client. Fails for an unregistered method or one this
    /// peer may not call.
    pub fn call(
        &self,
        session: &NetSession,
        object: &str,
        method: &str,
        args: &[u8],
    ) -> io::Result<()> {
        let me = session.id();
        let reliability = self
            .check(me, object, method)
            .map_err(|reason| invalid(&reason))?;
        let bytes = encode(object, method, args)?;
        match reliability {
            Reliability::Reliable => session.broadcast(&bytes),
            Reliability::Unreliable => session.broadcast_unreliable(&bytes),
        }
    }

    /// Reads a message `from` a peer. Returns `None` for a message that is
    /// not an RPC, and an error for a call that fails its checks, which the
    /// host can log or answer by kicking the sender.
    pub fn accept(
        &self,
        from: PeerId,
        bytes: &[u8],
    ) -> Option<Result<RpcCall, String>> {
        let body = bytes.strip_prefix(MAGIC)?;
        Some(decode(body).and_then(|(object, method, args)| {
            self.check(from, &object, &method)?;
            Ok(RpcCall {
                from,
                object,
                method,
                args,
            })
        }))
    }

    fn check(
        &self,
        caller: PeerId,
        object: &str,
        method: &str,
    ) -> Result<Reliability, String> {
        let (authority, reliability) = *self
            .methods
            .get(method)
            .ok_or_else(|| format!("unknown method {method:?}"))?;
        let allowed = caller == HOST
            || match authority {
                Authority::Host => false,
                Authority::Owner => self.owner(object) == caller,
                Authority::Anyone => true,
            };
        if allowed {
            Ok(reliability)
        } else {
            Err(format!(
                "peer {caller} may not call {method:?} on {object:?}"
            ))
        }
    }
}

/// MAGIC, then the object and method, each as a u16 little-endian length
/// and UTF-8, then the arguments.
fn encode(object: &str, method: &str, args: &[u8]) -> io::Result<Vec<u8>> {
    let mut bytes = MAGIC.to_vec();
    for name in [object, method] {
        let length = u16::try_from(name.len())
            .map_err(|_| invalid("object and method names fit 64 KiB"))?;
        bytes.extend(length.to_le_bytes());
        bytes.extend(name.as_bytes());
    }
    bytes.extend(args);
    Ok(bytes)
}

fn decode(mut body: &[u8]) -> Result<(String, String, Vec<u8>), String> {
    let mut name = || {
        let (length, rest) = body.split_at_checked(2).ok_or("short call")?;
        let length = usize::from(u16::from_le_bytes([length[0], length[1]]));
        let (name, rest) = rest.split_at_checked(length).ok_or("short call")?;
        body = rest;
        String::from_utf8(name.to_vec()).map_err(|_| "name is not UTF-8")
    };
    let object = name()?;
    let method = name()?;
    Ok((object, method, body.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::NetEvent;

    fn message(event: NetEvent) -> (PeerId, Vec<u8>) {
        match event {
            NetEvent::Message { from, bytes } => (from, bytes),
            other => panic!("expected a message, got {other:?}"),
        }
    }

    #[test]
    fn the_host_checks_who_may_call_each_method() {
        let (host, clients) = NetSession::loopback(2);
        host.poll();
        let mut rpcs = Rpcs::new();
        rpcs.register("jump", Authority::Owner, Reliability::Reliable)
            .register("wave", Authority::Anyone, Reliability::Unreliable)
            .register("explode", Authority::Host, Reliability::Reliable);
        rpcs.set_owner("Raft 1", 1);

        rpcs.call(&clients[0], "Raft 1", "jump", b"high").unwrap();
        let (from, bytes) = message(host.poll().remove(0));
        assert_eq!(
            rpcs.accept(from, &bytes),
            Some(Ok(RpcCall {
                from: 1,
                object: "Raft 1".into(),
                method: "jump".into(),
                args: b"high".to_vec(),
            }))
        );

        // The caller's own check refuses; a forged call fails on the host.
        assert!(rpcs.call(&clients[1], "Raft 1", "jump", b"").is_err());
        let forged = encode("Raft 1", "jump", b"").unwrap();
        assert!(matches!(rpcs.accept(2, &forged), Some(Err(_))));
        assert!(matches!(
            rpcs.accept(2, &encode("x", "explode", b"").unwrap()),
            Some(Err(_))
        ));
        assert!(matches!(
            rpcs.accept(1, &encode("x", "fly", b"").unwrap()),
            Some(Err(_))
        ));
        assert!(matches!(rpcs.accept(1, b"\0rpc\x09"), Some(Err(_))));
        assert!(rpcs
            .accept(2, &encode("Raft 1", "wave", b"").unwrap())
            .unwrap()
            .is_ok());

        // Host calls reach every client; game messages are not RPCs.
        rpcs.call(&host, "Raft 1", "explode", b"").unwrap();
        for client in &clients {
            let (from, bytes) = message(client.poll().remove(0));
            assert_eq!(
                rpcs.accept(from, &bytes).unwrap().unwrap().method,
                "explode"
            );
        }
        assert_eq!(rpcs.accept(1, b"plain game bytes"), None);
    }
}
