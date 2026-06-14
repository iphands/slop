//! End-to-end codec round-trips exercising the public q2proto surface together.
//!
//! These are integration tests (external to the crate), so they only use the public
//! API — validating that the modules compose into a usable client codec.

use q2proto::{
    oob_payload, tokenize, write_oob, ClcOp, InfoString, Reader, SvcOp, Usercmd, Writer,
    OOB_MARKER, OOB_PREFIX, PROTOCOL_VERSION,
};

#[test]
fn clc_move_round_trips() {
    // A client→server clc_move: opcode byte + a delta usercmd (from a null baseline).
    let from = Usercmd::default();
    let cmd = Usercmd {
        msec: 16,
        buttons: 1, // BUTTON_ATTACK
        angles: [1000, -2000, 0],
        forwardmove: 100,
        sidemove: -50,
        upmove: 0,
        impulse: 0,
        lightlevel: 2,
    };

    let mut w = Writer::new();
    w.write_u8(ClcOp::Move.into());
    cmd.write_delta(&mut w, &from);
    let bytes = w.freeze();

    let mut r = Reader::new(&bytes);
    assert_eq!(r.read_u8().unwrap(), u8::from(ClcOp::Move));
    let got = Usercmd::read_delta(&mut r, &from).unwrap();
    assert_eq!(got, cmd);
    assert_eq!(r.remaining(), 0);
}

#[test]
fn connect_packet_round_trips() {
    let mut info = InfoString::new();
    info.set("name", "qbots");
    info.set("rate", "25000");

    let qport: i32 = 0x1234;
    let challenge: i32 = 98765;
    let line = format!(
        "connect {} {} {} \"{}\"\n",
        PROTOCOL_VERSION,
        qport,
        challenge,
        info.as_str()
    );

    let mut w = Writer::new();
    write_oob(&mut w, &line);
    let bytes = w.freeze();

    // Server side: detect the marker, read the command line, tokenize.
    assert_eq!(bytes[..4], OOB_PREFIX);
    let payload = oob_payload(&bytes).unwrap();
    let argv = tokenize(std::str::from_utf8(payload).unwrap());
    assert_eq!(argv[0], "connect");
    assert_eq!(argv[1].parse::<i32>().unwrap(), PROTOCOL_VERSION);
    assert_eq!(argv[2].parse::<i32>().unwrap(), qport);
    assert_eq!(argv[3].parse::<i32>().unwrap(), challenge);

    // The userinfo round-trips back through InfoString parsing.
    let parsed = InfoString::from_raw(&argv[4]);
    assert_eq!(parsed.get("name").as_deref(), Some("qbots"));
    assert_eq!(parsed.get("rate").as_deref(), Some("25000"));

    // And the marker reads as the i32 -1.
    let mut r = Reader::new(&bytes);
    assert_eq!(r.read_i32().unwrap(), OOB_MARKER);
}

#[test]
fn server_frame_header_parses() {
    // A minimal svc_frame-style header: opcode + serverframe + deltaframe.
    let mut w = Writer::new();
    w.write_u8(SvcOp::Frame.into());
    w.write_i32(1000); // serverframe
    w.write_i32(-1); // deltaframe (no delta)
    let bytes = w.freeze();

    let mut r = Reader::new(&bytes);
    assert_eq!(SvcOp::from_u8(r.read_u8().unwrap()), Some(SvcOp::Frame));
    assert_eq!(r.read_i32().unwrap(), 1000);
    assert_eq!(r.read_i32().unwrap(), -1);
}

#[test]
fn proto_version_is_34() {
    assert_eq!(PROTOCOL_VERSION, 34);
}
