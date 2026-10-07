//! Inventory, from `rusting recipe apply inventory`.
//!
//! Touching an object in class `item` removes it and adds one to the counter
//! `inventory_<kind>`, where the kind is the first word of its name in lower
//! case: `Key 1` adds to `inventory_key`. Touching an object in class
//! `locked` while holding a key spends the key and removes that object.
//! Wire it up with `mod inventory;` in `src/main.rs` and
//! `inventory::inventory(scene);` in `update`.

use rusting_engine::prelude::*;

/// The counter that holds how many items of `kind` the player carries.
#[must_use]
pub fn counter(kind: &str) -> String {
    format!("inventory_{kind}")
}

/// How many items of `kind` the player carries.
#[must_use]
pub fn count(scene: &GameScene<'_>, kind: &str) -> i32 {
    scene.counter_or(&counter(kind), 0)
}

/// Spends one item of `kind`; `false`, and nothing spent, when there is none.
pub fn take(scene: &mut GameScene<'_>, kind: &str) -> bool {
    if count(scene, kind) <= 0 {
        return false;
    }
    scene.add_to_counter(&counter(kind), -1);
    true
}

/// Runs the inventory recipe once a frame.
pub fn inventory(scene: &mut GameScene<'_>) {
    let items = scene.in_class("item");
    let locked = scene.in_class("locked");
    for name in scene.touching("Player") {
        if items.contains(&name) {
            let kind = name.split_whitespace().next().unwrap_or("item");
            scene.add_to_counter(&counter(&kind.to_lowercase()), 1);
            scene.despawn(&name);
        } else if locked.contains(&name) && take(scene, "key") {
            scene.despawn(&name);
        }
    }
}
