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
- `set_master_volume(volume)` sets the main track.

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
  `/clips/sfx~1hit.wav`.
- `/level`: `[left, right]` RMS of the offline mix over the last tick.
- `/playing`: every sound that has not ended, with `clip`, `volume`, `pan`,
  `bus`, `looped` and the `tick` it starts on.

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

`rusting asset import` prints a WAV's `channels`: 1 is mono, which cannot
carry a stereo pan.
