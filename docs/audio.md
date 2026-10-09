# Audio

Sounds play through kira. Clip paths are relative to the project's
`assets/` folder (`"sfx/hit.wav"`). WAV, Ogg, MP3 and FLAC load.

## Playing sounds

```rust
use rusting_engine::prelude::*;

let hit = scene.play_sound("sfx/hit.wav", 0.8);
scene.play_sound_looped("music/hum.ogg", 0.5);
scene.play_sound_with("sfx/rock.wav", Sound {
    volume: 0.7,
    pan: -0.5,                    // -1 left .. 1 right
    bus: "sfx".into(),            // "" plays on the main track
    position: Some([3.0, 0.0, 1.0]), // or a world position instead of pan
    ..Sound::default()
});
scene.stop_sound(hit);
```

- `position` makes the active camera the listener. The sound pans to its
  side, and its volume falls as `2 / distance` past 2 m.
- `set_sound_volume(id, volume, fade)` moves one sound's volume over
  `fade` seconds.
- `set_bus_volume(bus, volume, fade)` does the same for a whole bus. To
  duck the music under an alarm: `set_bus_volume("music", 0.3, 0.2)`, then
  back to 1 when the alarm ends.
- `mute_bus(bus, true)` silences a bus and `false` brings it back at the
  volume it had. `solo_bus(bus, true)` plays only soloed buses while any
  is soloed; the main bus `""` is never silenced by solo. Scenarios read
  both under `audio:/buses/<name>/muted` and `/solo`.
- `set_master_volume(volume)` sets the main track.
- `restart()` and `load_scene(...)` stop no sound. A loop started in a
  `once` block would play twice after a restart, so call
  `stop_all_sounds()` (or `stop_sound(id)` on the loop) first.

## Speed and pitch

`rate` changes speed and pitch together, like a tape: 0.5 plays at half
speed an octave lower and takes twice as long; 2 is double speed.

```rust
let voice = scene.play_sound_with("bears/hello.ogg", Sound {
    rate: 1.6,          // a child's voice from an adult recording
    ..Sound::default()
});
scene.set_sound_rate(voice, 0.4, 2.0); // the tape winds down over 2 s
```

`reverse: true` plays the clip backwards from its end. A reversed clip is
always loaded whole, never streamed.

## Pause, resume and seek

```rust
scene.pause_sound(tape);
scene.resume_sound(tape);      // continues where it paused
scene.seek_sound(tape, 12.5);  // seconds into the clip
```

`pause_sounds(Some("music"))` pauses every sound on a bus, and
`pause_sounds(None)` every sound, for a pause menu. `resume_sounds` takes
the same argument and resumes only the sounds that are paused.

`scene.playing_sounds()` lists the sounds started and not yet ended,
oldest first, each with `id`, `clip`, `bus`, `looped` and `paused`. Use it
to check for leaked loops. It follows the audio device, which ends a sound
when its clip finishes, so do not branch the simulation on it: a headless
replay has no device, and there a sound stays listed until it is stopped.

In scenarios, `/playing` reports each sound's `position` (seconds into
the clip), `paused`, and `remaining` (real seconds left at the current
rate, `null` when looped).

## Moving sounds and the listener

A sound with `position` is heard from the listener: it pans to the
listener's side and falls off as `2 / distance` past 2 m. `Sound::falloff`
picks another curve: `Falloff::InverseSquare { near }` dies out faster,
`Falloff::Linear { near, far }` goes silent at `far`, and `Falloff::Off`
keeps the same volume at any distance. The listener is
the active camera unless you name one:

```rust
// The booth stays the ear while a monitor camera fills the screen.
scene.set_listener(Some("Booth"));
scene.set_listener(None); // back to the active camera

// Moves with its object every frame, like footsteps.
let steps = scene.play_sound_on("Big Button", "mascot/steps.ogg", Sound::default());
// Or move a sound yourself.
scene.set_sound_position(id, [3.0, 1.0, -8.0]);
```

`Sound { follow: Some(entity), .. }` does the same as `play_sound_on` from
system code.

`Sound { doppler: 1.0, .. }` bends a positioned sound's pitch like a
passing siren: higher while the distance to the listener shrinks, lower
while it grows (a car passing at 30 m/s plays about 1.1 then 0.92 times
its rate). 0.5 is half the effect; 0, the default, turns it off. The
factor stays between 0.5 and 2, and `/playing/<n>/rate` reports the rate
with doppler included.

A named listener stays fixed: it does not turn when another camera becomes
active. If the player switches to a camera that looks somewhere else, a
sound in front of that camera can come out hard left or right. Name a
listener only when it should not follow the view, and use
`set_listener(None)` otherwise.

Panning is constant power. At pan 0 each side plays at the sound's volume.
At pan 1 the right side plays at √2 times the volume (+3 dB) and the left
side is silent, so a hard-panned sound is louder on its side than at the
centre and can clip there first. `audio:/playing/N/gain` shows the
`[left, right]` gain of each sound, before bus and master volume.

## Occlusion

`occlude: true` raycasts from the listener to the sound every frame. When a
physics collider is in the way, the sound drops to 30% volume and through
an 800 Hz low-pass filter, so a bear behind a shelf sounds muffled.
`/playing/<n>/occlusion` is 0 when clear and 1 when blocked. Colliders on
the listener's and the followed object's own entities do not count.

## Bus effects

Every bus, and the main track (`""`), has a low-pass filter, a reverb and a
distortion, all off until you set them. Setting an effect again replaces
its settings; the fade is in seconds.

```rust
// Muffle the shelf bears and put them in a big room.
scene.set_bus_effect("bears", BusEffect::LowPass { cutoff_hz: 900.0 }, 0.5);
scene.set_bus_effect("bears", BusEffect::Reverb { room: 0.85, damping: 0.4, mix: 0.35 }, 0.0);
// Break up the radio.
scene.set_bus_effect("radio", BusEffect::Distortion { drive: 18.0, mix: 0.6 }, 0.1);
// Off again.
scene.set_bus_effect("bears", BusEffect::LowPass { cutoff_hz: 20_000.0 }, 0.5);
```

- `LowPass`: `cutoff_hz`; 20 000 or more is off.
- `Reverb`: `room` 0 to 1 is how long the tail rings, `damping` 0 to 1
  dulls it, `mix` 0 dry to 1 wet.
- `Distortion`: `drive` in decibels, `mix` 0 to 1. A soft clip.

An effect with `mix` 0 is off. `/buses/<bus>/effects` lists the effects
that are on, distortion first, then reverb, then the filter: the order the
signal goes through them.

### Reverb zones

A `rusting.reverb_zone` on an entity with a sensor collider puts a reverb
on its bus while the listener is inside the collider, and fades it out
when the listener leaves:

```json
"collider": {"shape": {"Box": {"half_extents": [6, 3, 10]}}, "sensor": true},
"rusting.reverb_zone": {"bus": "world", "room": 0.85, "damping": 0.4, "mix": 0.35, "fade": 0.5}
```

An empty `bus` is every sound, music included, so put world sounds on
their own bus. Where zones overlap, the first in entity order wins. A zone
owns the reverb on its bus: a `set_bus_effect` reverb there is turned off
when the listener leaves.

## Voice limits

A bus plays at most 64 sounds at once; `set_bus_voice_limit(bus, n)`
changes that, up to 256. Past the limit, a new sound replaces the playing
sound with the lowest `priority` (0 to 255, default 128), then the
quietest, counting distance and occlusion. When the new sound ranks lowest
itself, it is dropped. Give the mascot `priority: 255` and the shelf
chorus a lower one, and a thousand bears cannot drown it out.

When the bus is full:

- Priority compares only within one bus. A full `chorus` bus never stops
  a sound on `voice`, whatever their priorities.
- "Quietest" is the volume after distance falloff and occlusion, so with
  equal priority the farthest sound goes first.
- A new sound must rank strictly higher to replace one. On a full tie the
  playing sound stays and the new one is dropped (counted in `dropped`).
- Among playing sounds that tie for lowest, the oldest is replaced
  (counted in `stolen`).

`/buses/<bus>` reports `voices`, `limit`, `dropped` and `stolen`;
`/dropped` is the total of dropped and stolen sounds on every bus.

## Captions

```rust
scene.play_sound_with("bears/song.ogg", Sound {
    captions: vec![
        Caption::new(0.0, 1.5, "[bear, singing] One little bear..."),
        Caption::new(1.5, 3.0, "...went to sleep."),
    ],
    ..Sound::default()
});
scene.set_captions(true, 26.0); // on, 26 logical pixels
```

Caption times are seconds into the clip, so they follow `rate`, pause and
seek. A paused sound's caption hides until `resume_sound`. The HUD shows the current lines at the bottom center (`ui` feature).
`CaptionSettings { enabled, size }` is the resource behind the settings
toggle. `/captions` lists the lines showing now.

A `rusting.sound_cue` with a `caption` such as `[glass breaks]` shows it for
2 s each time it fires. Captions stay up for their whole time even when the
clip is shorter.

Set `full_volume_speed` on a sound cue to make collision sounds follow how
hard the body hits: a hit closing at that speed (m/s) or faster plays at the
cue's `volume`, a slower one plays quieter in proportion. A crate dropped
from 1 m thuds softly; from 6 m it plays loud. `trigger()` from game code
always plays at full `volume`.

## Long files

In a window, files over 1 MiB stream from disk while they play instead of
loading whole, so 5-minute ambience tracks do not stall a load. Scenario
mixes load every file whole, so they stay deterministic.
`/playing/<n>/streamed` reports which way a sound plays.

## Timing

Every sound belongs to a fixed tick. It starts one fixed step after that
tick's real time, whatever the frame rate, so sounds keep time with
gameplay. `at_tick: Some(t)` starts it on a later tick instead.

For music that stays on the beat, play one bar at a time with `at_tick` on
each bar's first tick. A long loop drifts against the fixed clock, and a
per-bar start does not.

`BeatClock` turns a tempo into ticks:

```rust
let clock = BeatClock::new(128, start_tick, time.fixed_delta);
if clock.is_beat(time.fixed_tick) { /* flash the metronome */ }
let next = time.fixed_tick + clock.ticks_to_next(time.fixed_tick);
scene.play_sound_with("music/bar.ogg", Sound {
    bus: "music".into(),
    at_tick: Some(next),
    ..Sound::default()
});
```

`tick_of(beat)` is the tick a beat starts on, rounded up. `beat_at(tick)`
is `None` before beat 0. `phase` goes from 0 to 1 within a beat.
`beats_between(after, upto)` lists the beats an update that ran several
ticks passed over.

## Judging input finer than a tick

`scene.press_tick(action)` gives the press time in fractional fixed ticks:
`121.4` is 40% of a tick after tick 121. A window takes the key or mouse
event's own time. Scenario presses and gamepads give the frame's tick.
Compare it with `clock.tick_of(beat) as f64` to judge early or late.

## Testing sound in scenarios

Headless runs have no audio device. Read the entity `audio:` instead:

- `/requested`: how many plays game code and sound cues asked for.
- `/clips/<clip>`: plays of one clip. Write `/` in the clip path as `~1`:
  `/clips/sfx~1hit.wav`. A clip that never played reads as 0, so
  `"equals": 0` checks that a sound never played.
- `/level`: `[left, right]` RMS of the offline mix over the last tick.
- `/peak`: `[left, right]` largest sample over the last tick, and
  `/clipped`: how many samples went past full scale.
- `/playing`: every sound that has not ended, with `id`, `clip`, `volume`,
  `pan`, `gain` (`[left, right]`), `bus`, `looped`, the `tick` it starts on, `rate`, `position`,
  `remaining`, `paused`, `priority`, `world_position`, `occlusion` and
  `streamed`. `greater_than` and `less_than` on an array compare its
  length: `{"entity": "audio:", "path": "/playing", "greater_than": 2}`
  means at least three sounds are playing.
- `/buses/<bus>`: `voices`, `limit`, `dropped`, `stolen`, `effects`, and
  the bus's own `level` and `peak` over the last tick, measured after its
  effects and its `set_bus_volume`. The main bus is `""`, so its level is
  `/buses//level`: the whole mix before the master volume. To prove a
  voice line is heard over the ambience, compare the two buses in one tick:
  `{"entity": "audio:", "path": "/buses/voice/level/0", "greater_than": 0.2}`
  and `{"entity": "audio:", "path": "/buses/ambience/level/0", "less_than": 0.08}`.
- `/dropped`: sounds the voice limits dropped or replaced.
- `/captions`: caption lines showing now.

```json
{"name": "rock pans left", "ticks": 30, "audio_out": "mix.wav",
 "steps": [
   {"tick": 5, "tap": "drop"},
   {"tick": 8, "expect": {"entity": "audio:", "path": "/playing/0/bus", "equals": "sfx"}},
   {"tick": 9, "log": {"entity": "audio:", "path": "/level"}}
 ]}
```

`audio_out` writes the whole mix as a 48 kHz stereo WAV, relative to the
scenario file, so you can listen to a run afterwards. The level and the
file come from kira's own mixer, so buses, fades, pans and scheduled starts
show in them. A start lands up to 1 ms early, because kira mixes in chunks.

### Reference mixes

`audio_reference` compares the whole mix with a stored WAV, the way
`golden` compares images. It fails when the RMS of the sample-by-sample
difference is over `tolerance` (default 0.01). `rusting test
--update-golden` writes the reference.

```json
{"name": "lullaby mix", "ticks": 240,
 "audio_reference": {"path": "golden/lullaby.wav", "tolerance": 0.005},
 "steps": [{"tick": 1, "tap": "start_tape"}]}
```

`rusting asset import` prints a WAV's `channels`: 1 is mono, which cannot
carry a stereo pan.
