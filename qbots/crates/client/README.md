# client — Connection + Frame Loop

**One tokio task per bot: the full Q2 client lifecycle from `getchallenge` to
`clc_move` heartbeat.**

> **The Loop:** Connect → receive frames → parse entities → run brain → send
> `usercmd` → repeat. Every 100ms (10 Hz).

---

## What This Is

`client` orchestrates the **network connection** and **frame parsing** for a single
bot:

- **Connection FSM** — `getchallenge` → `connect` → `client_connect` → active.
- **Netchan** — reliable/unreliable sequence-numbered channel over UDP.
- **Frame Parsing** — `svc_*` opcodes, `configstrings`, `playerstate`, entities.
- **Command Transmission** — `clc_move` with delta-compressed `usercmd`.

Built on `q2proto` for the wire codec.

---

## Quick Start

### Connecting

```rust
use client::{Conn, ConnState};
use tokio::net::UdpSocket;

// Create a connection
let mut conn = Conn::new(addr, "botname", 28000);

// Step 1: Send getchallenge
if let Some(pkt) = conn.start() {
    sock.send(&pkt).await?;
}

// Step 2: Receive challenge and send connect
let buf = sock.recv(&mut buf).await?;
if let Some(response) = conn.on_recv(&buf[..n]) {
    sock.send(&response).await?; // sends connect packet
}
```

### The Bot Loop

```rust
// Receive and send happen concurrently via tokio::select
loop {
    tokio::select! {
        // Receive server packets
        res = sock.recv(&mut buf) => {
            if let Some(response) = conn.on_recv(&buf[..n]) {
                sock.send(&response).await?;
            }
        }
        // Send usercmd every 100ms
        _ = ticker.tick() => {
            if conn.state() == ConnState::Active {
                let cmd = build_usercmd(); // from brain
                if let Some(pkt) = conn.transmit_cmd(&cmd) {
                    sock.send(&pkt).await?;
                }
            }
        }
    }
}
```

**⚠️ Don't call `transmit_cmd()` more than ~10 Hz.** The server will drop packets or kick you.
Use `tokio::time::interval(Duration::from_millis(100))` to pace your sends.

See [`src/conn.rs`](src/conn.rs) for the main loop, [`src/netchan.rs`](src/netchan.rs)
for the channel.

### From Connection to Bot

1. **Parse frames** → `conn.frame` gives you playerstate + visible entities.
2. **Build a world model** → use `world` crate to load the BSP + nav graph.
3. **Implement a brain** → see `brain` crate for combat/navigation FSM.
4. **Run a fleet** → see `qbots/src/main.rs` for multi-bot supervision.

See `qbots/src/main.rs:bot_task` for the full integration with the brain layer.

---

## Core Concepts

### Connection Handshake

The Q2 client handshake is **connectionless** (OOB packets):

1. **C→S:** `getchallenge\n`
2. **S→C:** `challenge %i p=34`
3. **C→S:** `connect 34 <qport> <challenge> "<userinfo>"`
4. **S→C:** `client_connect` → then `svc_serverdata`

See [`src/conn.rs`](src/conn.rs) for the state machine.

### Netchan

The **reliable channel** over UDP:

- **Sequence numbers** — detect lost/duplicate packets.
- **Acknowledgments** — retransmit unacked messages.
- **Fragmentation** — large messages split across packets.

Implemented in [`src/netchan.rs`](src/netchan.rs), ported from `netchan.c`.

### Frame Parsing

Each server frame contains:

- **`svc_serverdata`** — initial handshake (protocol, spawncount, gamedir).
- **`svc_configstring`** — indexed string table (maps, models, sounds).
- **`svc_frame`** — playerstate + entity deltas.
- **`svc_print` / `svc_sound`** — events (chat, footsteps, deaths).

See [`src/parse.rs`](src/parse.rs) for the opcode dispatcher.

### Userinfo

The `userinfo` string (`key\value\key\value`) is sent in the `connect` packet:

```rust
let mut userinfo = Userinfo::new();
userinfo.set("name", "botname");
userinfo.set("skin", "male/grunt");
userinfo.set("rate", "25000");
// ...
```

See [`src/userinfo.rs`](src/userinfo.rs).

---

## The Bot Loop

```rust
loop {
    // 1. Receive server packets
    tokio::select! {
        res = sock.recv(&mut buf) => {
            conn.on_recv(&buf[..n]);
        }
        // 2. Send usercmd every 100ms
        _ = ticker.tick() => {
            let cmd = brain.tick(...);
            conn.transmit_cmd(&cmd);
        }
    }
}
```

See `qbots/src/main.rs:bot_task` for the full implementation.

---

## State Machine

```
Disconnected
  ↓ (send getchallenge)
ChallengeSent
  ↓ (receive challenge)
ConnectSent
  ↓ (receive client_connect + svc_serverdata)
Active  (runs frame loop + sends usercmds)
  ↓ (disconnect or error)
Disconnected
```

---

## Testing

```bash
cargo test  # Connection FSM, frame parsing, userinfo encoding
```

Integration tests connect to a live Yamagi Q2 server.

---

## Sources

| Feature | yquake2 Source |
|---------|----------------|
| Handshake | `client/cl_network.c`, `server/sv_conless.c` |
| Netchan | `common/netchan.c` |
| Frame parsing | `client/cl_parse.c` |
| Configstrings | `client/cl_main.c` |

---

## License

MIT / Apache-2.0 (same as the rest of qbots).
