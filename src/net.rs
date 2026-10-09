//! Reliable multiplayer messages over TCP: a listen-server host, clients
//! that join it by address, and a relay that pairs hosts and clients by a
//! short room code so neither side needs an open port.
//!
//! Every peer talks only to the host (a star). The host has [`HOST`] as its
//! id; clients get ids from 1 in join order. Messages are byte strings that
//! arrive whole and in order; put JSON or bincode in them.
//!
//! ```no_run
//! use rusting_engine::net::{NetEvent, NetSession, HOST};
//! let host = NetSession::host(7777)?;
//! let client = NetSession::join("127.0.0.1:7777")?;
//! client.send(HOST, b"hello")?;
//! for event in host.poll() {
//!     if let NetEvent::Message { from, bytes } = event {
//!         host.send(from, &bytes)?;
//!     }
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Store the session as a resource from game code with
//! `scene.world().insert_resource(session)` and read it back each frame
//! with `scene.world().get_resource::<NetSession>()`.

use bevy_ecs::prelude::Resource;
use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A peer in a session. The host is [`HOST`]; clients count up from 1.
pub type PeerId = u32;
/// The host's id.
pub const HOST: PeerId = 0;
/// Wire protocol version. A host or relay refuses other versions.
pub const PROTOCOL_VERSION: u16 = 1;
/// Largest message, in bytes. Larger frames close the connection.
pub const MAX_MESSAGE: usize = 16 << 20;

const EVERYONE: PeerId = u32::MAX;
// Frame kinds.
const DATA: u8 = 0;
const JOINED: u8 = 1;
const LEFT: u8 = 2;
const HOST_ROOM: u8 = 3;
const ROOM: u8 = 4;
const JOIN: u8 = 5;
const WELCOME: u8 = 6;
const REJECT: u8 = 7;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Something that happened since the last [`NetSession::poll`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetEvent {
    /// A client joined (host only).
    Connected(PeerId),
    /// A client left (host only).
    Disconnected(PeerId),
    Message {
        from: PeerId,
        bytes: Vec<u8>,
    },
    /// The host or relay connection closed; the session is over.
    Closed,
}

/// One end of a multiplayer session. See the [module docs](self).
#[derive(Resource)]
pub struct NetSession {
    me: PeerId,
    room: Option<String>,
    route: Route,
    events: Mutex<Receiver<NetEvent>>,
    /// A direct host's listening address and its stop flag.
    listener: Option<(SocketAddr, Arc<AtomicBool>)>,
}

enum Route {
    /// Streams by peer: a direct host's clients, or a client's host.
    Direct(Arc<Mutex<BTreeMap<PeerId, TcpStream>>>),
    /// One stream to a relay; frames name the other end.
    Relay(Mutex<TcpStream>),
}

impl NetSession {
    /// Hosts on `port` of every interface.
    pub fn host(port: u16) -> io::Result<Self> {
        Self::host_on(TcpListener::bind(("0.0.0.0", port))?)
    }

    /// Hosts on an already bound listener, such as one on port 0 for a
    /// free port.
    pub fn host_on(listener: TcpListener) -> io::Result<Self> {
        let address = listener.local_addr()?;
        let (sender, events) = channel();
        let peers = Arc::new(Mutex::new(BTreeMap::new()));
        let accepted = Arc::clone(&peers);
        let closed = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&closed);
        std::thread::spawn(move || {
            let mut next = HOST + 1;
            for stream in listener.incoming() {
                // Dropping the session sets `stop` and connects once to
                // wake this loop, which frees the port.
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                // ponytail: handshakes run on the accept thread, so a
                // silent client delays the next join by up to 5 seconds.
                if handshake_client(&stream, next).is_err() {
                    continue;
                }
                let Ok(writer) = stream.try_clone() else {
                    continue;
                };
                accepted.lock().unwrap().insert(next, writer);
                if sender.send(NetEvent::Connected(next)).is_err() {
                    return;
                }
                let peers = Arc::clone(&accepted);
                let sender = sender.clone();
                let peer = next;
                std::thread::spawn(move || {
                    read_frames(
                        stream,
                        |kind, _, bytes| {
                            (kind == DATA).then_some(NetEvent::Message {
                                from: peer,
                                bytes,
                            })
                        },
                        &sender,
                    );
                    peers.lock().unwrap().remove(&peer);
                    let _ = sender.send(NetEvent::Disconnected(peer));
                });
                next += 1;
            }
        });
        Ok(Self {
            me: HOST,
            room: None,
            route: Route::Direct(peers),
            events: Mutex::new(events),
            listener: Some((address, closed)),
        })
    }

    /// Joins a host at `address`, such as `"192.168.1.20:7777"`.
    pub fn join(address: impl ToSocketAddrs) -> io::Result<Self> {
        let stream = TcpStream::connect(address)?;
        let me = request_join(&stream, "")?;
        let (sender, events) = channel();
        let reader = stream.try_clone()?;
        std::thread::spawn(move || {
            read_frames(reader, data_from_frame, &sender);
            let _ = sender.send(NetEvent::Closed);
        });
        Ok(Self {
            me,
            room: None,
            route: Route::Direct(Arc::new(Mutex::new(BTreeMap::from([(
                HOST, stream,
            )])))),
            events: Mutex::new(events),
            listener: None,
        })
    }

    /// Hosts through the relay at `relay` (see [`run_relay`]). The relay
    /// hands out a room code, [`NetSession::room_code`], for clients to
    /// join with.
    pub fn host_room(relay: impl ToSocketAddrs) -> io::Result<Self> {
        let mut stream = TcpStream::connect(relay)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
        write_frame(
            &mut stream,
            HOST_ROOM,
            HOST,
            &PROTOCOL_VERSION.to_le_bytes(),
        )?;
        let code = match read_frame(&mut stream)? {
            (ROOM, _, code) => String::from_utf8_lossy(&code).into_owned(),
            (REJECT, _, reason) => return Err(rejected(&reason)),
            _ => return Err(invalid("the relay did not answer with a room")),
        };
        stream.set_read_timeout(None)?;
        let (sender, events) = channel();
        let reader = stream.try_clone()?;
        std::thread::spawn(move || {
            read_frames(
                reader,
                |kind, peer, bytes| match kind {
                    DATA => Some(NetEvent::Message { from: peer, bytes }),
                    JOINED => Some(NetEvent::Connected(peer)),
                    LEFT => Some(NetEvent::Disconnected(peer)),
                    _ => None,
                },
                &sender,
            );
            let _ = sender.send(NetEvent::Closed);
        });
        Ok(Self {
            me: HOST,
            room: Some(code),
            route: Route::Relay(Mutex::new(stream)),
            events: Mutex::new(events),
            listener: None,
        })
    }

    /// Joins the room `code` on the relay at `relay`. Codes ignore case.
    pub fn join_room(
        relay: impl ToSocketAddrs,
        code: &str,
    ) -> io::Result<Self> {
        let stream = TcpStream::connect(relay)?;
        let code = code.trim().to_ascii_uppercase();
        let me = request_join(&stream, &code)?;
        let (sender, events) = channel();
        let reader = stream.try_clone()?;
        std::thread::spawn(move || {
            read_frames(reader, data_from_frame, &sender);
            let _ = sender.send(NetEvent::Closed);
        });
        Ok(Self {
            me,
            room: Some(code),
            route: Route::Relay(Mutex::new(stream)),
            events: Mutex::new(events),
            listener: None,
        })
    }

    /// This end's id: [`HOST`] for the host.
    #[must_use]
    pub fn id(&self) -> PeerId {
        self.me
    }

    /// The relay room code, if the session goes through a relay.
    #[must_use]
    pub fn room_code(&self) -> Option<&str> {
        self.room.as_deref()
    }

    /// Clients connected now, in id order (host only; empty on a client).
    #[must_use]
    pub fn peers(&self) -> Vec<PeerId> {
        match &self.route {
            Route::Direct(peers) if self.me == HOST => {
                peers.lock().unwrap().keys().copied().collect()
            }
            _ => Vec::new(),
        }
    }

    /// Sends `bytes` to `to`. A client can send only to [`HOST`].
    pub fn send(&self, to: PeerId, bytes: &[u8]) -> io::Result<()> {
        if self.me != HOST && to != HOST {
            return Err(invalid("clients send only to the host"));
        }
        match &self.route {
            Route::Direct(peers) => {
                let mut peers = peers.lock().unwrap();
                let stream = peers
                    .get_mut(&to)
                    .ok_or_else(|| invalid("no such peer"))?;
                write_frame(stream, DATA, self.me, bytes)
            }
            Route::Relay(stream) => {
                write_frame(&mut *stream.lock().unwrap(), DATA, to, bytes)
            }
        }
    }

    /// Sends `bytes` to every client (host), or to the host (client).
    pub fn broadcast(&self, bytes: &[u8]) -> io::Result<()> {
        if self.me != HOST {
            return self.send(HOST, bytes);
        }
        match &self.route {
            // A client that fails to take the message is dropped by its
            // reader thread, which reports Disconnected.
            Route::Direct(peers) => {
                for stream in peers.lock().unwrap().values_mut() {
                    let _ = write_frame(stream, DATA, HOST, bytes);
                }
                Ok(())
            }
            Route::Relay(stream) => {
                write_frame(&mut *stream.lock().unwrap(), DATA, EVERYONE, bytes)
            }
        }
    }

    /// Events since the last call, in arrival order. Never blocks.
    pub fn poll(&self) -> Vec<NetEvent> {
        self.events.lock().unwrap().try_iter().collect()
    }
}

impl Drop for NetSession {
    fn drop(&mut self) {
        if let Some((address, closed)) = &self.listener {
            closed.store(true, Ordering::Relaxed);
            let wake = if address.ip().is_unspecified() {
                SocketAddr::from(([127, 0, 0, 1], address.port()))
            } else {
                *address
            };
            let _ = TcpStream::connect_timeout(&wake, HANDSHAKE_TIMEOUT);
        }
        // Shutting the sockets down ends the reader threads.
        match &self.route {
            Route::Direct(peers) => {
                for stream in peers.lock().unwrap().values() {
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
            Route::Relay(stream) => {
                let _ = stream.lock().unwrap().shutdown(Shutdown::Both);
            }
        }
    }
}

/// Runs a relay on `listener` until it fails: hosts ask it for a room code
/// with [`NetSession::host_room`] and clients join that code with
/// [`NetSession::join_room`]. The relay forwards host messages to clients
/// and client messages to the host. `rusting relay` runs one.
// ponytail: one lock over every room serialises forwarding; per-room locks
// and a writer thread per socket if one relay serves many busy rooms.
pub fn run_relay(listener: TcpListener) -> io::Result<()> {
    let rooms = Arc::new(Mutex::new(HashMap::<String, RelayRoom>::new()));
    for stream in listener.incoming() {
        let stream = stream?;
        let rooms = Arc::clone(&rooms);
        std::thread::spawn(move || relay_connection(stream, &rooms));
    }
    Ok(())
}

struct RelayRoom {
    host: TcpStream,
    clients: BTreeMap<PeerId, TcpStream>,
    next: PeerId,
}

type Rooms = Mutex<HashMap<String, RelayRoom>>;

fn relay_connection(mut stream: TcpStream, rooms: &Rooms) {
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = stream.set_nodelay(true);
    let Ok((kind, _, payload)) = read_frame(&mut stream) else {
        return;
    };
    if let Err(reason) = check_version(&payload) {
        let _ = write_frame(&mut stream, REJECT, HOST, reason.as_bytes());
        return;
    }
    let _ = stream.set_read_timeout(None);
    match kind {
        HOST_ROOM => relay_host(stream, rooms),
        JOIN => {
            let code = String::from_utf8_lossy(&payload[2..]).into_owned();
            relay_client(stream, rooms, code);
        }
        _ => {}
    }
}

fn relay_host(mut stream: TcpStream, rooms: &Rooms) {
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let code = {
        let mut rooms = rooms.lock().unwrap();
        let code = loop {
            let code = room_code();
            if !rooms.contains_key(&code) {
                break code;
            }
        };
        rooms.insert(
            code.clone(),
            RelayRoom {
                host: writer,
                clients: BTreeMap::new(),
                next: HOST + 1,
            },
        );
        code
    };
    if write_frame(&mut stream, ROOM, HOST, code.as_bytes()).is_ok() {
        while let Ok((kind, to, bytes)) = read_frame(&mut stream) {
            if kind != DATA {
                continue;
            }
            let mut rooms = rooms.lock().unwrap();
            let Some(room) = rooms.get_mut(&code) else {
                break;
            };
            for (&peer, client) in &mut room.clients {
                if to == EVERYONE || to == peer {
                    let _ = write_frame(client, DATA, HOST, &bytes);
                }
            }
        }
    }
    // The room ends with its host.
    if let Some(room) = rooms.lock().unwrap().remove(&code) {
        for client in room.clients.values() {
            let _ = client.shutdown(Shutdown::Both);
        }
    }
}

fn relay_client(mut stream: TcpStream, rooms: &Rooms, code: String) {
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let peer = {
        let mut rooms = rooms.lock().unwrap();
        let Some(room) = rooms.get_mut(&code) else {
            drop(rooms);
            let reason = format!("no room {code}");
            let _ = write_frame(&mut stream, REJECT, HOST, reason.as_bytes());
            return;
        };
        let peer = room.next;
        room.next += 1;
        if write_frame(&mut stream, WELCOME, peer, &[]).is_err()
            || write_frame(&mut room.host, JOINED, peer, &[]).is_err()
        {
            return;
        }
        room.clients.insert(peer, writer);
        peer
    };
    while let Ok((kind, _, bytes)) = read_frame(&mut stream) {
        if kind != DATA {
            continue;
        }
        let mut rooms = rooms.lock().unwrap();
        let Some(room) = rooms.get_mut(&code) else {
            break;
        };
        let _ = write_frame(&mut room.host, DATA, peer, &bytes);
    }
    let mut rooms = rooms.lock().unwrap();
    if let Some(room) = rooms.get_mut(&code) {
        room.clients.remove(&peer);
        let _ = write_frame(&mut room.host, LEFT, peer, &[]);
    }
}

/// Six letters and digits without look-alikes (no I, O, 0 or 1).
fn room_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    uuid::Uuid::new_v4().as_bytes()[..6]
        .iter()
        .map(|byte| char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()]))
        .collect()
}

/// Host side of a direct join: reads JOIN, answers WELCOME or REJECT.
fn handshake_client(stream: &TcpStream, peer: PeerId) -> io::Result<()> {
    let mut stream = stream;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let (kind, _, payload) = read_frame(&mut stream)?;
    if kind != JOIN {
        return Err(invalid("expected a join"));
    }
    if let Err(reason) = check_version(&payload) {
        write_frame(&mut stream, REJECT, HOST, reason.as_bytes())?;
        return Err(invalid(&reason));
    }
    stream.set_read_timeout(None)?;
    write_frame(&mut stream, WELCOME, peer, &[])
}

/// Client side of a join; returns the id the host or relay assigned.
fn request_join(stream: &TcpStream, code: &str) -> io::Result<PeerId> {
    let mut stream = stream;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let mut payload = PROTOCOL_VERSION.to_le_bytes().to_vec();
    payload.extend(code.as_bytes());
    write_frame(&mut stream, JOIN, HOST, &payload)?;
    let me = match read_frame(&mut stream)? {
        (WELCOME, me, _) => me,
        (REJECT, _, reason) => return Err(rejected(&reason)),
        _ => return Err(invalid("the host did not answer the join")),
    };
    stream.set_read_timeout(None)?;
    Ok(me)
}

fn check_version(payload: &[u8]) -> Result<(), String> {
    match payload {
        [low, high, ..]
            if u16::from_le_bytes([*low, *high]) == PROTOCOL_VERSION =>
        {
            Ok(())
        }
        [low, high, ..] => Err(format!(
            "protocol version {} is not {PROTOCOL_VERSION}",
            u16::from_le_bytes([*low, *high])
        )),
        _ => Err("missing protocol version".into()),
    }
}

fn data_from_frame(kind: u8, peer: PeerId, bytes: Vec<u8>) -> Option<NetEvent> {
    (kind == DATA).then_some(NetEvent::Message { from: peer, bytes })
}

/// Reads frames until the stream ends, sending the events `event` makes.
fn read_frames(
    mut stream: TcpStream,
    event: impl Fn(u8, PeerId, Vec<u8>) -> Option<NetEvent>,
    sender: &Sender<NetEvent>,
) {
    while let Ok((kind, peer, bytes)) = read_frame(&mut stream) {
        if let Some(event) = event(kind, peer, bytes) {
            if sender.send(event).is_err() {
                break;
            }
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}

/// A frame is a little-endian u32 length of the rest, a kind byte, a
/// little-endian u32 peer id and the payload.
fn write_frame(
    stream: &mut impl Write,
    kind: u8,
    peer: PeerId,
    payload: &[u8],
) -> io::Result<()> {
    if payload.len() > MAX_MESSAGE {
        return Err(invalid("message is larger than MAX_MESSAGE"));
    }
    let mut frame = Vec::with_capacity(9 + payload.len());
    frame.extend(u32::try_from(5 + payload.len()).unwrap().to_le_bytes());
    frame.push(kind);
    frame.extend(peer.to_le_bytes());
    frame.extend(payload);
    stream.write_all(&frame)
}

fn read_frame(stream: &mut impl Read) -> io::Result<(u8, PeerId, Vec<u8>)> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if !(5..=MAX_MESSAGE + 5).contains(&length) {
        return Err(invalid("bad frame length"));
    }
    let mut frame = vec![0; length];
    stream.read_exact(&mut frame)?;
    let peer = PeerId::from_le_bytes(frame[1..5].try_into().unwrap());
    Ok((frame[0], peer, frame.split_off(5)))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn rejected(reason: &[u8]) -> io::Error {
    io::Error::new(
        io::ErrorKind::ConnectionRefused,
        String::from_utf8_lossy(reason).into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Polls until `count` events arrive or two seconds pass.
    fn wait(session: &NetSession, count: usize) -> Vec<NetEvent> {
        let start = Instant::now();
        let mut events = Vec::new();
        while events.len() < count && start.elapsed() < Duration::from_secs(2) {
            events.extend(session.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        events
    }

    fn message(from: PeerId, text: &str) -> NetEvent {
        NetEvent::Message {
            from,
            bytes: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn a_direct_host_and_two_clients_exchange_messages_in_order() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = NetSession::host_on(listener).unwrap();
        let first = NetSession::join(address).unwrap();
        assert_eq!(wait(&host, 1), [NetEvent::Connected(1)]);
        let second = NetSession::join(address).unwrap();
        assert_eq!(wait(&host, 1), [NetEvent::Connected(2)]);
        assert_eq!((first.id(), second.id()), (1, 2));
        assert_eq!(host.peers(), [1, 2]);

        first.send(HOST, b"a").unwrap();
        first.send(HOST, b"b").unwrap();
        assert_eq!(wait(&host, 2), [message(1, "a"), message(1, "b")]);
        assert!(first.send(2, b"x").is_err(), "clients reach only the host");

        host.broadcast(b"tick").unwrap();
        host.send(2, b"only you").unwrap();
        assert_eq!(wait(&first, 1), [message(HOST, "tick")]);
        assert_eq!(
            wait(&second, 2),
            [message(HOST, "tick"), message(HOST, "only you")]
        );

        drop(first);
        assert_eq!(wait(&host, 1), [NetEvent::Disconnected(1)]);
        drop(host);
        assert_eq!(wait(&second, 1), [NetEvent::Closed]);
        // The port is free again for the next match.
        let start = Instant::now();
        while TcpListener::bind(address).is_err() {
            assert!(start.elapsed() < Duration::from_secs(2), "port held");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_relay_pairs_a_host_and_a_client_by_room_code() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let relay = listener.local_addr().unwrap();
        std::thread::spawn(move || run_relay(listener));
        let host = NetSession::host_room(relay).unwrap();
        let code = host.room_code().unwrap().to_owned();
        assert_eq!(code.len(), 6);

        let error = NetSession::join_room(relay, "NOROOM").err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
        let client =
            NetSession::join_room(relay, &code.to_lowercase()).unwrap();
        assert_eq!(client.id(), 1);
        assert_eq!(wait(&host, 1), [NetEvent::Connected(1)]);

        client.send(HOST, b"ping").unwrap();
        assert_eq!(wait(&host, 1), [message(1, "ping")]);
        host.send(1, b"pong").unwrap();
        host.broadcast(b"all").unwrap();
        assert_eq!(
            wait(&client, 2),
            [message(HOST, "pong"), message(HOST, "all")]
        );

        drop(client);
        assert_eq!(wait(&host, 1), [NetEvent::Disconnected(1)]);
    }

    #[test]
    fn a_host_refuses_another_protocol_version() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let _host = NetSession::host_on(listener).unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        write_frame(&mut stream, JOIN, HOST, &99u16.to_le_bytes()).unwrap();
        let (kind, _, reason) = read_frame(&mut stream).unwrap();
        assert_eq!(kind, REJECT);
        assert!(String::from_utf8_lossy(&reason).contains("99"));
    }
}
