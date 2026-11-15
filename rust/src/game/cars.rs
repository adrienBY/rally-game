//! The garage's cars: names, the numbers shown in the garage, and how each one changes the handling. Bodies are built by the
//! page (CAR_SPECS in web/rally_template.html, same order); every car shares the wheels, so the physics geometry is the same.
use crate::physics::Tune;

pub struct CarSpec {
    pub name: &'static str, pub class: &'static str, pub drive: &'static str,
    /// shown as bars, 0..10: top speed, acceleration, handling, grip
    pub stats: [f64; 4],
    /// engine power, front/rear tyre grip, front share of the drive, arcade yaw response and side grip, brakes (multipliers of the
    /// stage tune; split is absolute)
    pub power: f64, pub grip_f: f64, pub grip_r: f64, pub split: f64, pub yaw: f64, pub side: f64, pub brake: f64,
    /// engine note: multiplies the revs the sound engine is given
    pub pitch: f64,
}

pub const CARS: [CarSpec; 5] = [
    CarSpec { name: "BRUME R4", class: "MODERN HOT HATCH", drive: "AWD", stats: [6.0, 6.0, 8.0, 7.0],
        power: 1.0, grip_f: 1.0, grip_r: 1.0, split: 0.5, yaw: 1.0, side: 1.0, brake: 1.0, pitch: 1.0 },
    CarSpec { name: "FOUDRE 80", class: "80S MID-ENGINE WEDGE", drive: "REAR-BIASED", stats: [9.0, 9.0, 5.0, 5.0],
        power: 1.2, grip_f: 1.02, grip_r: 0.93, split: 0.28, yaw: 1.14, side: 0.86, brake: 0.95, pitch: 0.88 },
    CarSpec { name: "GRANITE 4X4", class: "90S TURBO SALOON", drive: "AWD", stats: [7.0, 7.0, 7.0, 9.0],
        power: 1.06, grip_f: 1.05, grip_r: 1.07, split: 0.5, yaw: 0.94, side: 1.12, brake: 1.03, pitch: 0.95 },
    CarSpec { name: "LUCIOLE 74", class: "70S LIGHTWEIGHT COUPE", drive: "RWD", stats: [4.0, 5.0, 9.0, 4.0],
        power: 0.86, grip_f: 1.0, grip_r: 0.9, split: 0.0, yaw: 1.12, side: 0.8, brake: 0.92, pitch: 1.12 },
    CarSpec { name: "ORAGE H1", class: "HYBRID PROTOTYPE", drive: "AWD", stats: [10.0, 8.0, 7.0, 8.0],
        power: 1.14, grip_f: 1.06, grip_r: 1.08, split: 0.45, yaw: 1.0, side: 1.1, brake: 1.1, pitch: 1.06 },
];

/// the stage tune with car k's character
pub fn tune_for(base: &Tune, k: usize) -> Tune {
    let c = &CARS[k.min(CARS.len() - 1)]; let mut t = *base;
    t.power *= c.power; t.a_accel *= c.power; t.grip_f *= c.grip_f; t.grip_r *= c.grip_r; t.split = c.split;
    t.a_yaw *= c.yaw; t.a_grip *= c.side; t.brake *= c.brake; t.a_brake *= c.brake; t
}
/// which body an AI rival drives offline (each rival its own, so a full grid shows the whole family)
pub fn rival_model(r: usize) -> usize { (r + 1) % CARS.len() }
