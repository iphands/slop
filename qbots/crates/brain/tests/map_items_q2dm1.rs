//! Plan 74 T2: `build_map_items` must tell a weapon pad from an ammo pad.
//!
//! Both are [`EntityClass::ItemWeapon`]; only `MapItem::weapon` separates them. q2dm1's entity
//! lump holds 11 `weapon_*` and 35 `ammo_*` spawns (counted from `pak0.pak`, Plan 74 T1).
//!
//! Gated on the Quake 2 pak being present (`vendor/baseq2` by default, or `QBOTS_BASEQ2`),
//! the same skip-and-pass pattern as `hazard_q2dm3.rs`.

use std::path::PathBuf;

use brain::items::build_map_items;
use brain::perception::EntityClass;
use brain::weapons::Weapon;
use world::{Bsp, NavGraph};

fn baseq2_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QBOTS_BASEQ2") {
        let pb = PathBuf::from(p);
        return pb.join("pak0.pak").exists().then_some(pb);
    }
    let pb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/baseq2");
    pb.join("pak0.pak").exists().then_some(pb)
}

#[test]
fn map_item_weapon_pads_are_not_ammo_pads_on_q2dm1() {
    let Some(baseq2) = baseq2_dir() else {
        eprintln!("[skip] q2dm1 pak not found (set QBOTS_BASEQ2 or populate vendor/baseq2)");
        return;
    };
    let bsp = Bsp::load(&baseq2, "q2dm1").expect("load q2dm1");
    // One node is enough: this test is about classification, not nav resolution.
    let graph = NavGraph::from_raw(vec![[0.0, 0.0, 0.0]], vec![Vec::new()]);
    let items = build_map_items(&bsp, &graph);

    let weapon_class: Vec<_> = items
        .iter()
        .filter(|i| i.class == EntityClass::ItemWeapon)
        .collect();
    let pads: Vec<Weapon> = weapon_class.iter().filter_map(|i| i.weapon).collect();
    let ammo = weapon_class.iter().filter(|i| i.weapon.is_none()).count();
    assert_eq!(pads.len(), 11, "weapon pads: {pads:?}");
    assert_eq!(ammo, 35, "ammo pads share ItemWeapon but carry no weapon");
    assert_eq!(
        pads.iter()
            .filter(|w| **w == Weapon::RocketLauncher)
            .count(),
        2
    );
    assert!(
        items
            .iter()
            .filter(|i| i.class != EntityClass::ItemWeapon)
            .all(|i| i.weapon.is_none()),
        "only weapon-class pads may name a weapon"
    );
}
