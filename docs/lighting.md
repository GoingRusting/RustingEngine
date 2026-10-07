# Lighting

Which lights the renderer has, how many it draws, and which ones cast
shadows. `rusting schema point_light` (and `spot_light`,
`directional_light`) gives each field with its units and typical values.

## Light types

| Light | Scene key | Shadows |
| --- | --- | --- |
| Sun | `directional_light` | Yes, with `"shadows": true`. Only the first shadowed directional light casts them. |
| Lamp | `point_light` | No. |
| Cone | `spot_light` | Yes, with `"shadows": true`, when no directional light casts them. |
| Flat fill | `rusting.ambient_light` | No; it lights everything equally. |
| Sky and ground fill | `rusting.sky_light` | No; up-facing surfaces get `sky_color`, down-facing ones `ground_color`. |

Point lights pass through walls: a lamp lights the room behind a wall as far
as its `range` reaches. Keep each `range` inside its room and put lamps near
room centres. A spot light with `"shadows": true` stops at walls, so a
flashlight does not light the far side of a door. One light per frame casts
shadows: the first directional light with `shadows`, or, when there is none,
the first spot light with `shadows`. A mesh's `cast_shadows` and
`receive_shadows` apply to that one shadow.

## How many lights draw

The renderer draws at most 64 lights a frame at `High` (and `Auto`)
quality, 32 at `Balanced` and 16 at `Eco` (`/render/quality` in the scene).
Ambient and sky light do not count.

Past the cap, lights are dropped in this order: directional lights are kept
first, then point lights, then spot lights; within each kind, lights are
taken in entity order, which is the order objects were created in. So a
flashlight spot light is the first thing to go in a building with many
lamps. Hidden objects (`visible: false`, or under a hidden parent) do not
count, so hiding lamps in rooms the player is not in keeps the count down.

`rusting capture --json` (`data.render`) and `rusting test --json`
(`perf.render` of each scenario that renders) give the dropped count as
`dropped_lights`; zero means every light drew.
