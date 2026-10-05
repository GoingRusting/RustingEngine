//! Retargeting: copies a clip from one character's skeleton to another's.
//! Bones pair up by humanoid name (`Animation::humanoid`) or, without a
//! map, by bone name with rig prefixes such as `mixamorig:` dropped. Each
//! rotation key keeps its turn away from the source's rest pose, measured
//! in the rig's frame, so rigs whose bones point along different axes
//! still move alike. Only the hips keep position keys, scaled by hip
//! height. Rest poses are the transforms in the scene, so retarget a
//! scene that is not playing.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::World;
use nalgebra::{Quaternion, UnitQuaternion, Vector3};
use serde::{Deserialize, Serialize};

use super::{
    find_target, Animation, AnimationClip, AnimationProperty, AnimationTrack,
    Children, Keyframe, Name,
};
use crate::Transform;

/// One entry of a skeleton's humanoid map: a standard bone name such as
/// `Hips`, `Spine`, `LeftUpperArm` or `RightFoot`, and this rig's path to it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HumanoidBone {
    pub bone: String,
    pub path: String,
}

/// Rest pose of a bone: local rotation, rotation and position in the
/// frame of the animated object, and local position.
struct Rest {
    local: UnitQuaternion<f32>,
    rig: UnitQuaternion<f32>,
    height: f32,
    position: Vector3<f32>,
}

fn rest(world: &World, root: Entity, path: &str) -> Option<Rest> {
    let mut rig = UnitQuaternion::identity();
    let mut at = Vector3::zeros();
    let mut entity = root;
    let mut transform = Transform::default();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        entity = find_target(world, entity, part)?;
        transform = *world.get::<Transform>(entity)?;
        at += rig * Vector3::from(transform.position);
        rig *= rotation(&transform);
    }
    Some(Rest {
        local: rotation(&transform),
        rig,
        height: at.y,
        position: Vector3::from(transform.position),
    })
}

fn rotation(transform: &Transform) -> UnitQuaternion<f32> {
    let [x, y, z] = transform.rotation;
    UnitQuaternion::from_euler_angles(x, y, z)
}

/// Lowercase bone name without a rig prefix (`mixamorig:`) or separators.
fn key(name: &str) -> String {
    let name = name.rsplit('/').next().unwrap_or(name);
    let name = name.rsplit(':').next().unwrap_or(name);
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The humanoid key of the bone at `path` on `root`.
fn bone_of(world: &World, root: Entity, path: &str) -> String {
    world
        .get::<Animation>(root)
        .and_then(|a| a.humanoid.iter().find(|b| b.path == path))
        .map_or_else(|| key(path), |b| key(&b.bone))
}

/// This rig's path to the bone with humanoid key `bone`.
fn path_of(world: &World, root: Entity, bone: &str) -> Option<String> {
    if let Some(entry) = world
        .get::<Animation>(root)
        .and_then(|a| a.humanoid.iter().find(|b| key(&b.bone) == bone))
    {
        return Some(entry.path.clone());
    }
    // Breadth first, children in order, so the shallowest match wins.
    let mut queue = std::collections::VecDeque::from([(root, String::new())]);
    while let Some((entity, path)) = queue.pop_front() {
        for &child in world.get::<Children>(entity).map_or(&[][..], |c| &c.0) {
            let Some(name) = world.get::<Name>(child) else {
                continue;
            };
            let path = if path.is_empty() {
                name.0.clone()
            } else {
                format!("{path}/{}", name.0)
            };
            if key(&name.0) == bone {
                return Some(path);
            }
            queue.push_back((child, path));
        }
    }
    None
}

/// `clip` of `from`'s animation, retargeted onto the skeleton under `to`.
/// Tracks on the animated object itself are kept as they are; bone tracks
/// other than rotations and hip position, and bones `to` lacks, are left
/// out.
pub fn retarget_clip(
    world: &World,
    from: Entity,
    clip: &str,
    to: Entity,
) -> Result<AnimationClip, String> {
    let animation = world
        .get::<Animation>(from)
        .ok_or("the source object has no rusting.animation")?;
    let source = animation
        .clip(clip)
        .map(|i| &animation.clips[i])
        .ok_or_else(|| format!("the source has no clip `{clip}`"))?;
    if !source.blend.is_empty() {
        return Err(format!(
            "`{clip}` is a blend space; retarget its point clips one by one"
        ));
    }
    let scale = match (
        path_of(world, from, "hips").and_then(|p| rest(world, from, &p)),
        path_of(world, to, "hips").and_then(|p| rest(world, to, &p)),
    ) {
        (Some(a), Some(b)) if a.height > 0.0 && b.height > 0.0 => {
            b.height / a.height
        }
        _ => 1.0,
    };
    let mut tracks = Vec::new();
    for track in &source.tracks {
        if track.target.is_empty() {
            tracks.push(track.clone());
            continue;
        }
        let bone = bone_of(world, from, &track.target);
        let Some(path) = path_of(world, to, &bone) else {
            continue;
        };
        let (Some(a), Some(b)) =
            (rest(world, from, &track.target), rest(world, to, &path))
        else {
            continue;
        };
        let (parent_a, parent_b) =
            (a.rig * a.local.inverse(), b.rig * b.local.inverse());
        let mut keys = Vec::with_capacity(track.keys.len());
        let property = match track.property {
            AnimationProperty::Rotation | AnimationProperty::Orientation => {
                let mut last: Option<UnitQuaternion<f32>> = None;
                for k in &track.keys {
                    let q = if track.property == AnimationProperty::Rotation {
                        let at = |i| k.value.get(i).copied().unwrap_or(0.0);
                        UnitQuaternion::from_euler_angles(at(0), at(1), at(2))
                    } else {
                        let at = |i| {
                            k.value.get(i).copied().unwrap_or(f32::from(i == 3))
                        };
                        UnitQuaternion::new_normalize(Quaternion::new(
                            at(3),
                            at(0),
                            at(1),
                            at(2),
                        ))
                    };
                    // The turn from rest in the rig frame, then back into
                    // the target bone's parent frame.
                    let turn = parent_a * q * a.rig.inverse();
                    let mut out = parent_b.inverse() * turn * b.rig;
                    if last
                        .is_some_and(|last| last.coords.dot(&out.coords) < 0.0)
                    {
                        out = UnitQuaternion::new_unchecked(-out.into_inner());
                    }
                    last = Some(out);
                    keys.push(Keyframe {
                        time: k.time,
                        value: vec![out.i, out.j, out.k, out.w],
                    });
                }
                AnimationProperty::Orientation
            }
            AnimationProperty::Position if bone == "hips" => {
                let turn = parent_b.inverse() * parent_a;
                for k in &track.keys {
                    let at = |i| k.value.get(i).copied().unwrap_or(0.0);
                    let moved = Vector3::new(at(0), at(1), at(2)) - a.position;
                    let p = b.position + turn * moved * scale;
                    keys.push(Keyframe {
                        time: k.time,
                        value: vec![p.x, p.y, p.z],
                    });
                }
                AnimationProperty::Position
            }
            _ => continue,
        };
        tracks.push(AnimationTrack {
            target: path,
            property,
            interpolation: track.interpolation,
            keys,
        });
    }
    Ok(AnimationClip {
        tracks,
        ..source.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{hierarchy::set_parent, Interpolation, TweenRepeat};

    fn bone(
        world: &mut World,
        name: &str,
        parent: Entity,
        t: Transform,
    ) -> Entity {
        let entity = world.spawn((Name(name.into()), t)).id();
        set_parent(world, entity, parent).unwrap();
        entity
    }

    fn at(position: [f32; 3], rotation: [f32; 3]) -> Transform {
        Transform {
            position,
            rotation,
            ..Transform::default()
        }
    }

    #[test]
    fn clips_move_a_differently_built_rig_alike() {
        use std::f32::consts::FRAC_PI_2;
        let mut world = World::new();
        // Source: hips 1 m up, arm bone pointing along its parent's x.
        let source = world
            .spawn((Name("Mixamo".into()), Transform::default()))
            .id();
        let hips = bone(
            &mut world,
            "mixamorig:Hips",
            source,
            at([0.0, 1.0, 0.0], [0.0; 3]),
        );
        bone(
            &mut world,
            "mixamorig:LeftArm",
            hips,
            at([0.2, 0.4, 0.0], [0.0; 3]),
        );
        // Target: hips 2 m up, arm bone turned a quarter about z at rest,
        // mapped by humanoid name.
        let target = world
            .spawn((Name("Robot".into()), Transform::default()))
            .id();
        let pelvis =
            bone(&mut world, "Pelvis", target, at([0.0, 2.0, 0.0], [0.0; 3]));
        bone(
            &mut world,
            "ArmL",
            pelvis,
            at([0.4, 0.8, 0.0], [0.0, 0.0, FRAC_PI_2]),
        );
        world.entity_mut(target).insert(Animation {
            humanoid: vec![
                HumanoidBone {
                    bone: "Hips".into(),
                    path: "Pelvis".into(),
                },
                HumanoidBone {
                    bone: "LeftArm".into(),
                    path: "Pelvis/ArmL".into(),
                },
            ],
            ..Animation::default()
        });
        let key = |time: f32, value: &[f32]| Keyframe {
            time,
            value: value.to_vec(),
        };
        let track = |target: &str, property, keys| AnimationTrack {
            target: target.into(),
            property,
            interpolation: Interpolation::Linear,
            keys,
        };
        world.entity_mut(source).insert(Animation {
            clips: vec![AnimationClip {
                name: "wave".into(),
                repeat: TweenRepeat::Loop,
                tracks: vec![
                    track(
                        "",
                        AnimationProperty::Scale,
                        vec![key(0.0, &[1.0; 3])],
                    ),
                    track(
                        "mixamorig:Hips",
                        AnimationProperty::Position,
                        vec![
                            key(0.0, &[0.0, 1.0, 0.0]),
                            key(1.0, &[0.5, 0.9, 0.0]),
                        ],
                    ),
                    track(
                        "mixamorig:Hips/mixamorig:LeftArm",
                        AnimationProperty::Rotation,
                        vec![key(0.0, &[0.0; 3]), key(1.0, &[0.0, 0.7, 0.0])],
                    ),
                    track(
                        "mixamorig:Hips/mixamorig:Ghost",
                        AnimationProperty::Rotation,
                        vec![],
                    ),
                ],
                ..AnimationClip::default()
            }],
            ..Animation::default()
        });
        let clip = retarget_clip(&world, source, "wave", target).unwrap();
        let targets: Vec<&str> =
            clip.tracks.iter().map(|t| t.target.as_str()).collect();
        assert_eq!(targets, ["", "Pelvis", "Pelvis/ArmL"]);
        // Hips move twice as far on a rig twice as tall.
        let hips = &clip.tracks[1].keys[1].value;
        assert!((hips[0] - 1.0).abs() < 1e-5 && (hips[1] - 1.8).abs() < 1e-5);
        // At rest the target arm keeps its own rest turn.
        let quaternion = |v: &[f32]| {
            UnitQuaternion::new_normalize(Quaternion::new(
                v[3], v[0], v[1], v[2],
            ))
        };
        let rest = UnitQuaternion::from_euler_angles(0.0, 0.0, FRAC_PI_2);
        let arm = &clip.tracks[2].keys;
        assert!(quaternion(&arm[0].value).angle_to(&rest) < 1e-5);
        // Swinging the source arm 0.7 rad about rig y swings the target's
        // arm the same way in the rig frame, whatever its own axes.
        let swing = UnitQuaternion::from_euler_angles(0.0, 0.7, 0.0);
        assert!(quaternion(&arm[1].value).angle_to(&(swing * rest)) < 1e-5);
        assert!(retarget_clip(&world, source, "missing", target).is_err());
    }
}
