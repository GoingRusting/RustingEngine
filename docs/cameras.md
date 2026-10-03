# Cameras

A scene can hold any number of `rusting.camera` entities. Only active
cameras render; an inactive camera costs nothing.

## Which camera fills the window

Among active cameras **without** a viewport, the highest `priority` fills
the window. On a tie, the one spawned first wins. That camera also aims
pointer clicks, `scene.pointer_ray()`, `expect_screen` and the
3D sound listener.

## Split screen and picture-in-picture

`viewport` is the part of the window a camera draws into, as fractions
`[x, y, width, height]` from the top-left corner. Every active camera with a
viewport draws after the full-window camera, lower priority first, so a
higher priority view lands on top.

Two players side by side:

```json
{"name": "P1 Camera", "components": {"rusting.camera": {
  "projection": {"Perspective": {"vertical_fov_radians": 1.0, "near": 0.1, "far": 200.0}},
  "active": true, "priority": 0, "viewport": [0.0, 0.0, 0.5, 1.0]}}}
{"name": "P2 Camera", "components": {"rusting.camera": {
  "projection": {"Perspective": {"vertical_fov_radians": 1.0, "near": 0.1, "far": 200.0}},
  "active": true, "priority": 1, "viewport": [0.5, 0.0, 0.5, 1.0]}}}
```

A minimap is one full-window camera plus a small viewport camera with a
higher priority, for example `[0.75, 0.0, 0.25, 0.25]`.

When every active camera has a viewport, the uncovered parts of the window
are black.

From game code:

```rust
scene.set_camera("P2 Camera", true, Some([0.5, 0.0, 0.5, 1.0])); // on, right half
scene.set_camera("P2 Camera", false, None);                       // off
scene.set_active_camera("Cutscene Camera"); // the only active camera
```

`set_camera` leaves other cameras alone. `set_active_camera` turns every
other camera off.

## Aiming a camera

Rotation is Euler angles in radians. X applies first, then Y, then Z.
Forward is -Z. `scene.basis(name)` returns the world `[right, up, forward]`
unit vectors of any object, parents included:

```rust
if let Some([_, _, forward]) = scene.basis("P1 Camera") {
    let eye = scene.object("P1 Camera").position();
    let hit = scene.raycast(eye, forward, 50.0);
}
```

`raycast_skipping(eye, dir, 50.0, &["glass"])` passes through objects in
those classes.

A `rusting.player_controller` owns the transform of its direct camera
children. It sets their pitch, and it sets their orbit offset when
`camera_distance > 0`. `camera_offset` moves the camera off the body, for an
over-the-shoulder view. `mouse_look: false` stops the controller from
capturing the mouse, so a second player can use it.

## HUD per camera

`"camera": "P2 Camera"` on a `rusting.hud` element anchors the element to
that camera's viewport. The element shows only while that camera is
active.

## Testing cameras in scenarios

- `expect_screen` with `"camera": "P2 Camera"` projects through that
  camera, active or not. `inside` and `min_share` are then fractions of its
  viewport.
- `capture` with `{"path": "p2.png", "camera": "P2 Camera"}` renders only
  that camera, over the whole frame and without the HUD.
- `expect_pixels` with `"camera"` checks a region of that camera's
  viewport in the real frame.
- With viewport cameras, `perf.render.cameras` lists each drawn camera
  with its `gpu_ms`, `draws` and `triangles`.

## Limits

- Pointer clicks, `pointer_ray()` and the sound listener use the
  full-window camera, even over another camera's viewport.
- Occlusion culling (`FrustumAndOcclusion`) keeps one depth history for all
  cameras. Use `Frustum` culling with split screen.
- Post effects (bloom, fog, tone mapping, render scale) apply to every
  camera alike. Per-camera effects are not supported.
- A camera cannot render into a texture for a material or an egui image.
  Ray-cast a small image in game code instead. Split Signal's CCTV feeds do
  this.
