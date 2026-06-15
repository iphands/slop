//! Movement controller — converts intent to `Usercmd`.
//!
//! Takes high-level intent (desired yaw/pitch + forward/side/up + jump/crouch/attack)
//! and converts it into a `Usercmd` that the movement controller consumes.
//! Uses Q2 movement constants from `pmove.c`.

use q2proto::Usercmd;

pub const MAX_SPEED: f32 = 320.0;
pub const JUMP_VELOCITY: f32 = 270.0;
pub const ACCEL: f32 = 10.0;

pub const BUTTON_ATTACK: u8 = 1;
pub const BUTTON_USE: u8 = 2;
pub const BUTTON_ANY: u8 = 128;

#[derive(Debug, Clone, Copy)]
pub struct MovementIntent {
    pub yaw: f32,
    pub pitch: f32,
    pub forward: f32,
    pub side: f32,
    pub up: f32,
    pub jump: bool,
    pub crouch: bool,
    pub attack: bool,
    pub weapon: Option<u8>,
}

impl MovementIntent {
    pub fn new() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            forward: 0.0,
            side: 0.0,
            up: 0.0,
            jump: false,
            crouch: false,
            attack: false,
            weapon: None,
        }
    }

    pub fn look_at(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch;
    }

    pub fn move_forward(&mut self, speed: f32) {
        self.forward = speed.clamp(-1.0, 1.0);
    }

    pub fn move_side(&mut self, speed: f32) {
        self.side = speed.clamp(-1.0, 1.0);
    }

    pub fn jump(&mut self) {
        self.jump = true;
    }

    pub fn attack(&mut self) {
        self.attack = true;
    }
}

impl Default for MovementIntent {
    fn default() -> Self {
        Self::new()
    }
}

/// Movement controller — converts intent to `Usercmd`.
#[derive(Debug)]
pub struct MovementController {
    last_cmd: Usercmd,
    msec: u8,
    /// Server-side angle offset (`pmove.delta_angles`). The server's actual
    /// view angle is `SHORT2ANGLE(cmd.angles[i] + delta_angles[i])`
    /// (`pmove.c:1255`), seeded on every spawn/respawn (`game/player/client.c:1675`)
    /// as `ANGLE2SHORT(spawn_angles - cmd_angles)`. A client doing *absolute*
    /// world-space aiming (like us) must **subtract** this offset when encoding
    /// a desired world angle, or every aim and movement direction is rotated by
    /// a constant `spawn_yaw` — bots walk into walls away from their target.
    delta_angles: [i16; 3],
}

impl MovementController {
    pub fn new() -> Self {
        Self {
            last_cmd: Usercmd::default(),
            msec: 33, // ~30 Hz
            delta_angles: [0, 0, 0],
        }
    }

    /// Update the server-side `delta_angles` from the latest playerstate.
    /// Call this every tick before `build_cmd`.
    pub fn set_delta_angles(&mut self, delta_angles: [i16; 3]) {
        self.delta_angles = delta_angles;
    }

    /// Convert intent to a `Usercmd`.
    pub fn build_cmd(&mut self, intent: MovementIntent) -> Usercmd {
        let mut cmd = Usercmd {
            msec: self.msec,
            angles: [
                self.angle_short(intent.pitch, 0),
                self.angle_short(intent.yaw, 1),
                0, // roll
            ],
            forwardmove: (intent.forward * MAX_SPEED) as i16,
            sidemove: (intent.side * MAX_SPEED) as i16,
            upmove: (intent.up * MAX_SPEED) as i16,
            impulse: intent.weapon.unwrap_or(0),
            lightlevel: 0, // TODO: get from environment
            buttons: 0,
        };

        if intent.jump {
            cmd.upmove = JUMP_VELOCITY as i16;
        }
        if intent.crouch {
            // Crouch is typically handled by viewoffset, not a button
        }
        if intent.attack {
            cmd.buttons |= BUTTON_ATTACK;
        }

        cmd.lightlevel = 255; // Assume full light for now

        self.last_cmd = cmd;
        cmd
    }

    /// Encode a desired *world-space* angle (degrees) into the i16 the server
    /// needs in `usercmd.angles[axis]`, accounting for `delta_angles`.
    ///
    /// The server computes `viewangles[axis] = SHORT2ANGLE(cmd.angles[axis] +
    /// delta_angles[axis])` in i16 wraparound arithmetic (`pmove.c:1255`). So to
    /// make the bot face `deg` we must send `ANGLE2SHORT(deg) - delta_angles`,
    /// all modulo 65536, then reinterpret as signed i16.
    ///
    /// Matches `ANGLE2SHORT(x) = (int)(x * 65536 / 360) & 65535` (`shared.h:1184`).
    fn angle_short(&self, deg: f32, axis: usize) -> i16 {
        let desired = ((deg * 65536.0 / 360.0).round() as i32).rem_euclid(65536);
        let delta = (self.delta_angles[axis] as i32).rem_euclid(65536);
        // (desired - delta) mod 65536, then bit-reinterpret as i16 exactly like
        // a C `short` stores the value.
        let val = ((desired + 65536 - delta) % 65536) as u16;
        val as i16
    }

    /// Get the last command sent.
    pub fn last_cmd(&self) -> &Usercmd {
        &self.last_cmd
    }
}

impl Default for MovementController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_movement_intent_creation() {
        let intent = MovementIntent::new();
        assert_eq!(intent.forward, 0.0);
        assert_eq!(intent.side, 0.0);
        assert!(!intent.attack);
    }

    #[test]
    fn test_movement_intent_methods() {
        let mut intent = MovementIntent::new();
        intent.move_forward(0.5);
        assert_eq!(intent.forward, 0.5);

        intent.move_side(-0.3);
        assert_eq!(intent.side, -0.3);

        intent.attack();
        assert!(intent.attack);
    }

    #[test]
    fn test_angle_short_no_delta_matches_angle2short() {
        let controller = MovementController::new();
        // With delta_angles = 0, angle_short must equal ANGLE2SHORT semantics.
        assert_eq!(controller.angle_short(0.0, 1), 0);
        assert_eq!(controller.angle_short(90.0, 1), 16384);
        assert_eq!(controller.angle_short(-90.0, 1), -16384);
        assert_eq!(controller.angle_short(270.0, 1), -16384); // == -90°
        assert_eq!(controller.angle_short(180.0, 1), -32768); // wraps to i16 min
        assert_eq!(controller.angle_short(360.0, 1), 0); // full circle
        assert_eq!(controller.angle_short(-360.0, 1), 0);
    }

    /// Regression for the spawn-yaw bug: if the server seeds `delta_angles[YAW]`
    /// so the bot spawns facing 90°, we must send a usercmd angle that makes the
    /// server's `SHORT2ANGLE(cmd + delta)` equal to our desired world yaw.
    #[test]
    fn angle_short_subtracts_delta_angles() {
        let mut controller = MovementController::new();
        // Spawn facing yaw=90° with cmd_angles=0 → delta = ANGLE2SHORT(90) = 16384.
        let spawn_yaw_delta: i16 = 16384;
        controller.set_delta_angles([0, spawn_yaw_delta, 0]);

        // To face world yaw 0, we send cmd.angle such that cmd + delta == 0.
        let cmd_yaw = controller.angle_short(0.0, 1);
        let reconstructed = (cmd_yaw as i32 + spawn_yaw_delta as i32) as i16;
        let world_yaw = reconstructed as f32 * (360.0 / 65536.0);
        let world_yaw = (world_yaw + 360.0).rem_euclid(360.0);
        assert!((world_yaw - 0.0).abs() < 0.1, "face 0: got {world_yaw}");

        // To face world yaw 90, send cmd such that cmd+delta == ANGLE2SHORT(90).
        let cmd_yaw = controller.angle_short(90.0, 1);
        let reconstructed = (cmd_yaw as i32 + spawn_yaw_delta as i32) as i16;
        let world_yaw = reconstructed as f32 * (360.0 / 65536.0);
        let world_yaw = (world_yaw + 360.0).rem_euclid(360.0);
        assert!((world_yaw - 90.0).abs() < 0.1, "face 90: got {world_yaw}");

        // To face world yaw 180.
        let cmd_yaw = controller.angle_short(180.0, 1);
        let reconstructed = (cmd_yaw as i32 + spawn_yaw_delta as i32) as i16;
        let world_yaw = reconstructed as f32 * (360.0 / 65536.0);
        let world_yaw = (world_yaw + 360.0).rem_euclid(360.0);
        assert!((world_yaw - 180.0).abs() < 0.1, "face 180: got {world_yaw}");
    }

    #[test]
    fn test_build_cmd() {
        let mut controller = MovementController::new();
        let mut intent = MovementIntent::new();
        intent.move_forward(1.0);
        intent.look_at(90.0, 0.0);
        intent.attack();

        let cmd = controller.build_cmd(intent);
        assert!(cmd.forwardmove > 0);
        assert!(cmd.buttons & BUTTON_ATTACK != 0);
    }

    #[test]
    fn test_msec() {
        let controller = MovementController::new();
        assert_eq!(controller.msec, 33);
    }
}
