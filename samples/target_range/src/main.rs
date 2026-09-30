use rusting_engine::prelude::*;

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    if !scene.pressed("fire") {
        return;
    }
    if let Some(mut shots) = scene.counter("shots") {
        shots.value += 1;
    }
    let Some(hit) = scene.aim(50.0) else {
        return;
    };
    if !hit.name.starts_with("Target") {
        return;
    }
    scene.despawn(&hit.name);
    if let Some(mut hits) = scene.counter("hits") {
        hits.value += 1;
    }
    scene.object("Hit Burst").set_position(hit.point);
    scene.trigger("Hit Burst");
}

rusting_game!(update);
