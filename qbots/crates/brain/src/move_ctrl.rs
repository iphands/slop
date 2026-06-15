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
}

impl MovementController {
    pub fn new() -> Self {
        Self {
            last_cmd: Usercmd::default(),
            msec: 33, // ~30 Hz
        }
    }

    /// Convert intent to a `Usercmd`.
    pub fn build_cmd(&mut self, intent: MovementIntent) -> Usercmd {
        let mut cmd = Usercmd {
            msec: self.msec,
            angles: [
                self.deg_to_short(intent.pitch),
                self.deg_to_short(intent.yaw),
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

    /// Convert degrees to Q2 angle format (signed short).
    /// Q2 reads these back as: `short_value * (360.0 / 65536.0)`.
    /// Mapping: 0°→0, 90°→16384, 180°→-32768, 270°→-16384.
    fn deg_to_short(&self, deg: f32) -> i16 {
        let normalized = deg.rem_euclid(360.0); // [0, 360)
        let raw = (normalized * 65536.0 / 360.0) as i32; // [0, 65536)
                                                         // Map to signed i16: [0,32768) stays positive, [32768,65536) wraps negative
        if raw >= 32768 {
            (raw - 65536) as i16
        } else {
            raw as i16
        }
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
    fn test_deg_to_short() {
        let controller = MovementController::new();
        assert_eq!(controller.deg_to_short(0.0), 0);
        assert_eq!(controller.deg_to_short(90.0), 16384);
        assert_eq!(controller.deg_to_short(-90.0), -16384);
        assert_eq!(controller.deg_to_short(270.0), -16384); // equivalent to -90°
        assert_eq!(controller.deg_to_short(180.0), -32768); // wraps to i16 min
        assert_eq!(controller.deg_to_short(360.0), 0); // full circle
        assert_eq!(controller.deg_to_short(-360.0), 0); // full circle negative
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
