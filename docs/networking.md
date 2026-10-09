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
    match NetSession::host(7777) {
        Ok(session) => scene.world().insert_resource(session),
        Err(error) => eprintln!("host failed: {error}"),
    }
}

fn start_client(scene: &mut GameScene<'_>, address: &str) {
    // A join waits up to 5 seconds for the host's answer.
    if let Ok(session) = NetSession::join(address) {
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
- Remove the resource (`scene.world().remove_resource::<NetSession>()`) to
  leave. Dropping a host session closes every client and frees the port.

## Room codes through a relay

When players are behind home routers, run a relay on a machine both can
reach:

```sh
rusting relay 0.0.0.0:7777
```

The host asks the relay for a room, and players join with the code:

```rust
let host = NetSession::host_room("relay.example.com:7777")?;
let code = host.room_code().unwrap(); // six characters, such as "K7QXRM"

let client = NetSession::join_room("relay.example.com:7777", "k7qxrm")?;
```

Codes ignore case and leave out the look-alikes I, O, 0 and 1. The room
closes when its host leaves. Game code is the same as for direct
sessions; only the constructor differs.

## Limits

- TCP only: no unreliable UDP channel yet, so a lost packet delays the
  messages behind it. Send state snapshots at a fixed rate rather than
  every frame.
- No replication, prediction or rollback is built in. Send what changed
  (positions, counters) and apply it on the other side.
- No encryption or authentication. Do not send secrets.
- The wire protocol has a version (`PROTOCOL_VERSION`); a host or relay
  refuses clients built with another one.
