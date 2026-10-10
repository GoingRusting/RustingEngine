# Networking

`rusting_engine::net` sends reliable, ordered messages between one host and
its clients over TCP. It uses only the standard library. Every client talks
to the host, and the host forwards what other clients need (a star).

- The host has id `HOST` (0). Clients get ids from 1 in join order.
- A message is a byte string. It arrives whole and in order. Put JSON
  (`serde_json::to_vec`) or bincode in it.
- `poll()` never blocks. Call it once per frame or tick.

## Host and join by address

For a LAN game or a host with an open port:

```rust
use rusting_engine::net::{NetEvent, NetSession, HOST};
use rusting_engine::prelude::*;

fn start_host(scene: &mut GameScene<'_>) {
    match NetSession::host(7777, "") {
        Ok(session) => scene.world().insert_resource(session),
        Err(error) => eprintln!("host failed: {error}"),
    }
}

fn start_client(scene: &mut GameScene<'_>, address: &str) {
    // A join waits up to 5 seconds for the host's answer.
    if let Ok(session) = NetSession::join(address, "") {
        scene.world().insert_resource(session);
    }
}

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    let Some(session) = scene.world().get_resource::<NetSession>() else {
        return;
    };
    for event in session.poll() {
        match event {
            NetEvent::Connected(peer) => { /* send the new player the world */ }
            NetEvent::Disconnected(peer) => { /* remove that player's raft */ }
            NetEvent::Message { from, bytes } => { /* apply it */ }
            NetEvent::Closed => { /* the host left: back to the menu */ }
        }
    }
}
```

- `send(peer, bytes)` sends to one peer. A client can send only to `HOST`.
- `broadcast(bytes)` sends to every client from the host, or to the host
  from a client.
- `peers()` lists the host's connected clients.
- Pass a password to `host` and the same one to `join` to keep strangers
  out: `NetSession::host(7777, "raft")`. A wrong one fails the join with
  `ConnectionRefused` and the message "wrong password". `""` means none.
- Remove the resource (`scene.world().remove_resource::<NetSession>()`) to
  leave. Dropping a host session closes every client and frees the port.

## Room codes through a relay

When players are behind home routers, run a relay on a machine both can
reach:

```sh
rusting relay 0.0.0.0:7777
```

To expose a relay on the internet, give it a token. Hosts and clients
without it are refused:

```sh
RUSTING_RELAY_TOKEN=long-random-string rusting relay 0.0.0.0:7777
```

The host asks the relay for a room, and players join with the code:

```rust
let host = NetSession::host_room("relay.example.com:7777", TOKEN)?;
let code = host.room_code().unwrap(); // six characters, such as "K7QXRM"

let client = NetSession::join_room("relay.example.com:7777", "k7qxrm", TOKEN)?;
```

Codes ignore case and leave out the look-alikes I, O, 0 and 1. The room
closes when its host leaves. Game code is the same as for direct
sessions; only the constructor differs.

## Testing a bad connection

`simulate` makes one end act as if its connection were slow or lossy, so a
game can be tried against lag on one machine:

```rust
use rusting_engine::net::NetConditions;
use std::time::Duration;

session.simulate(NetConditions {
    latency: Duration::from_millis(75), // each way: set it on both ends
    jitter: Duration::from_millis(20),
    loss: 0.02,
    seed: 1,
});
```

- It delays what `poll` returns on this end. Set it on the host and on a
  client for a 150 ms round trip.
- Messages stay reliable and in order. A lost message is resent after one
  more round trip (at least 200 ms), and the messages behind it wait too,
  as on real TCP.
- The same seed gives the same delays. `NetConditions::default()` turns
  it off.

`stats()` returns `NetStats`: messages and wire bytes sent and received
(payload plus a 9-byte frame header each), and `held`, the events the
simulation is holding back. Show it on a debug HUD to see what a game
sends per second.

## Tests without sockets

`NetSession::loopback(clients)` returns a host and that many clients joined
to it inside one process, with no ports or threads. They take the same
calls as real sessions: the host's first `poll` reports each client as
`Connected`, a dropped client is `Disconnected`, and a dropped host sends
`Closed` to every client. Unit-test game networking code with them.

```rust
use rusting_engine::net::{NetEvent, NetSession, HOST};

let (host, clients) = NetSession::loopback(2);
clients[0].send(HOST, b"ready")?;
for event in host.poll() {
    if let NetEvent::Message { from, bytes } = event {
        host.send(from, &bytes)?; // echo back
    }
}
```

## Limits

- TCP only: no unreliable UDP channel yet, so a lost packet delays the
  messages behind it. Send state snapshots at a fixed rate rather than
  every frame.
- No replication, prediction or rollback is built in. Send what changed
  (positions, counters) and apply it on the other side.
- No encryption. The password and relay token travel in plain text, so
  they keep strangers out but do not hide traffic from someone on the
  path. Do not send other secrets.
- The relay limits each IP address to 16 open connections and 60 new
  connections a minute (failed ones count, which slows token guessing),
  caps everyone together at 1024 connections, and reads at most 1 MiB/s
  from each connection. Still use a long random token. To change the
  limits, run the relay from Rust with `net::run_relay_with(listener,
  token, RelayLimits { .. })`.
- One player who connects and then sends nothing does not hold up anyone
  else's join; each handshake runs on its own thread and must finish
  within 5 seconds. The relay counts an IPv6 /64 as one address.
- Passwords and relay tokens must fit in a 64 KiB handshake.
- The wire protocol has a version (`PROTOCOL_VERSION`); a host or relay
  refuses clients built with another one.
