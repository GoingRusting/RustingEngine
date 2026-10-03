//! Sound requests. Presentation only: nothing here reads or writes
//! simulation state, and the queue is not part of state hashes.
//!
//! Game code and [`SoundCue`](super::SoundCue)s push [`AudioCommand`]s onto
//! the [`AudioQueue`]. The windowed runner plays them through the audio
//! device each frame (`audio` feature). Headless runs and `rusting test`
//! have no device: nothing drains the queue (it keeps the newest
//! [`QUEUE_LIMIT`] commands), and [`AudioQueue::requested`] still counts
//! sounds that game code asked for. The windowed runner and scenarios turn
//! `SoundCue` events into requests; scenarios read them through the
//! `audio:` entity (`/requested`, `/clips/<clip>`).

use bevy_ecs::prelude::{Resource, World};

use super::{EventQueue, SoundEvent};

/// Beats counted on fixed ticks with integer math, so a beat lands on the
/// same tick every run: beat `k` starts on the first tick at or after `k`
/// beats of real time. At 60 ticks a second, 120 BPM is every 30 ticks and
/// 128 BPM alternates 28 and 29.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeatClock {
    pub bpm: u32,
    /// Tick of beat 0.
    pub start_tick: u64,
    /// Fixed ticks in one minute: 3600 at 60 ticks a second.
    pub ticks_per_minute: u64,
}

impl BeatClock {
    /// `fixed_delta` is [`FrameTime::fixed_delta`](super::FrameTime).
    #[must_use]
    pub fn new(
        bpm: u32,
        start_tick: u64,
        fixed_delta: std::time::Duration,
    ) -> Self {
        Self {
            bpm: bpm.max(1),
            start_tick,
            ticks_per_minute: (60.0 / fixed_delta.as_secs_f64()).round() as u64,
        }
    }

    /// Tick beat `beat` starts on.
    #[must_use]
    pub fn tick_of(&self, beat: u64) -> u64 {
        let bpm = u64::from(self.bpm);
        self.start_tick + (beat * self.ticks_per_minute).div_ceil(bpm)
    }

    /// The beat playing on `tick`; `None` before beat 0.
    #[must_use]
    pub fn beat_at(&self, tick: u64) -> Option<u64> {
        let since = tick.checked_sub(self.start_tick)?;
        Some(since * u64::from(self.bpm) / self.ticks_per_minute)
    }

    /// Whether a beat starts on `tick`.
    #[must_use]
    pub fn is_beat(&self, tick: u64) -> bool {
        self.beat_at(tick)
            .is_some_and(|beat| self.tick_of(beat) == tick)
    }

    /// Ticks from `tick` until the next beat starts, 1 or more.
    #[must_use]
    pub fn ticks_to_next(&self, tick: u64) -> u64 {
        let next = self.beat_at(tick).map_or(0, |beat| beat + 1);
        self.tick_of(next) - tick
    }

    /// How far `tick` is into its beat, from 0 up to 1.
    #[must_use]
    pub fn phase(&self, tick: u64) -> f64 {
        let Some(since) = tick.checked_sub(self.start_tick) else {
            return 0.0;
        };
        let scaled = since * u64::from(self.bpm);
        (scaled % self.ticks_per_minute) as f64 / self.ticks_per_minute as f64
    }

    /// Beats that start after tick `after` and up to tick `upto`. An update
    /// that ran several fixed ticks loops over all of them.
    #[must_use]
    pub fn beats_between(&self, after: u64, upto: u64) -> std::ops::Range<u64> {
        let first = self.beat_at(after).map_or(0, |beat| beat + 1);
        let end = self.beat_at(upto).map_or(0, |beat| beat + 1);
        first..end.max(first)
    }
}

/// Commands kept when nothing drains the queue (headless runs).
pub const QUEUE_LIMIT: usize = 1024;

/// How a sound plays: [`GameScene::play_sound_with`](crate::project_runner::GameScene::play_sound_with).
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    /// Linear gain, 1 is the clip's own level.
    pub volume: f32,
    /// -1 is the left speaker only, 0 both, 1 the right only.
    pub pan: f32,
    /// Bus the sound plays on (`"music"`, `"sfx"`); empty is the main
    /// output. A bus is created the first time it is named.
    pub bus: String,
    pub looped: bool,
    /// Fixed tick the sound starts on. `None` is the tick it was asked for
    /// on. A later tick schedules it, so it starts exactly then.
    pub at_tick: Option<u64>,
    /// World position. The active camera is the listener: the position
    /// sets the pan and lowers the volume with distance.
    pub position: Option<[f32; 3]>,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            bus: String::new(),
            looped: false,
            at_tick: None,
            position: None,
        }
    }
}

/// Names one playing sound so it can be stopped later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SoundId(pub u64);

/// One request for the audio device.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioCommand {
    Play {
        id: SoundId,
        /// Path relative to the project's `assets` folder.
        clip: String,
        /// Linear gain, 1 is the clip's own level.
        volume: f32,
        looped: bool,
        /// -1 left to 1 right.
        pan: f32,
        /// Empty is the main output.
        bus: String,
        /// Fixed tick the sound starts on.
        tick: u64,
    },
    Stop(SoundId),
    /// Moves a playing sound's gain to `volume` over `fade` seconds.
    SetVolume {
        id: SoundId,
        volume: f32,
        fade: f32,
    },
    /// Moves a bus's gain to `volume` over `fade` seconds.
    SetBusVolume {
        bus: String,
        volume: f32,
        fade: f32,
    },
    StopAll,
    /// Linear gain applied to every sound.
    SetMasterVolume(f32),
}

/// Requests waiting for the audio device.
#[derive(Resource, Clone, Debug, Default)]
pub struct AudioQueue {
    commands: Vec<AudioCommand>,
    next_id: u64,
    requested: u64,
    /// Plays requested per clip path, never reset.
    clips: std::collections::BTreeMap<String, u64>,
}

impl AudioQueue {
    /// Asks for a clip to start on fixed tick `tick` (or `sound.at_tick`).
    /// `sound.position` must already be turned into pan and volume. Returns
    /// the ID that stops it.
    pub fn play(&mut self, clip: &str, sound: &Sound, tick: u64) -> SoundId {
        let id = SoundId(self.next_id);
        self.next_id += 1;
        self.requested += 1;
        *self.clips.entry(clip.to_owned()).or_default() += 1;
        self.push(AudioCommand::Play {
            id,
            clip: clip.to_string(),
            volume: sound.volume,
            looped: sound.looped,
            pan: sound.pan.clamp(-1.0, 1.0),
            bus: sound.bus.clone(),
            tick: sound.at_tick.unwrap_or(tick),
        });
        id
    }

    fn push(&mut self, command: AudioCommand) {
        if self.commands.len() >= QUEUE_LIMIT {
            self.commands.remove(0);
        }
        self.commands.push(command);
    }

    pub fn stop(&mut self, id: SoundId) {
        self.push(AudioCommand::Stop(id));
    }

    pub fn stop_all(&mut self) {
        self.push(AudioCommand::StopAll);
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        self.push(AudioCommand::SetMasterVolume(volume));
    }

    pub fn set_volume(&mut self, id: SoundId, volume: f32, fade: f32) {
        self.push(AudioCommand::SetVolume { id, volume, fade });
    }

    pub fn set_bus_volume(&mut self, bus: &str, volume: f32, fade: f32) {
        self.push(AudioCommand::SetBusVolume {
            bus: bus.to_owned(),
            volume,
            fade,
        });
    }

    /// Plays requested so far, played or not. Never reset.
    #[must_use]
    pub fn requested(&self) -> u64 {
        self.requested
    }

    /// Plays requested per clip path, played or not. Never reset.
    #[must_use]
    pub fn requested_clips(&self) -> &std::collections::BTreeMap<String, u64> {
        &self.clips
    }

    /// Takes the waiting commands; the runner calls this once per frame.
    pub fn drain(&mut self) -> Vec<AudioCommand> {
        std::mem::take(&mut self.commands)
    }
}

/// Turns the [`SoundEvent`]s visible this frame into play requests. The
/// runners call it once after each update; it is not a schedule system
/// because adding one changes system order, and replays hash that.
pub fn route_sound_events(world: &mut World) {
    let tick = world
        .get_resource::<crate::runtime::FrameTime>()
        .map_or(0, |time| time.fixed_tick);
    let clips: Vec<(String, f32)> = world
        .get_resource::<EventQueue<SoundEvent>>()
        .map(|events| {
            events.iter().map(|e| (e.clip.clone(), e.volume)).collect()
        })
        .unwrap_or_default();
    if let Some(mut queue) = world.get_resource_mut::<AudioQueue>() {
        for (clip, volume) in clips {
            let sound = Sound {
                volume,
                ..Sound::default()
            };
            queue.play(&clip, &sound, tick);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_clock_lands_on_whole_ticks() {
        let step = std::time::Duration::from_secs_f64(1.0 / 60.0);
        let clock = BeatClock::new(128, 2, step);
        let ticks: Vec<_> = (0..5).map(|beat| clock.tick_of(beat)).collect();
        assert_eq!(ticks, [2, 31, 59, 87, 115]);
        assert_eq!(clock.beat_at(1), None);
        assert_eq!(clock.beat_at(58), Some(1));
        assert!(clock.is_beat(59) && !clock.is_beat(60));
        assert_eq!(clock.ticks_to_next(59), 28);
        assert_eq!(clock.ticks_to_next(0), 2);
        assert_eq!(clock.beats_between(30, 90), 1..4);
        assert_eq!(clock.beats_between(0, 1), 0..0);
        let even = BeatClock::new(120, 0, step);
        assert_eq!(even.tick_of(3), 90);
        assert_eq!(even.phase(45), 0.5);
    }

    #[test]
    fn requests_get_distinct_ids_and_drain_once() {
        let mut queue = AudioQueue::default();
        let a = queue.play("sfx/a.wav", &Sound::default(), 0);
        let b = queue.play("sfx/b.wav", &Sound::default(), 0);
        queue.stop(a);
        assert_ne!(a, b);
        assert_eq!(queue.requested(), 2);
        assert_eq!(queue.requested_clips()["sfx/b.wav"], 1);
        assert_eq!(queue.drain().len(), 3);
        assert!(queue.drain().is_empty());
        for _ in 0..QUEUE_LIMIT + 10 {
            queue.set_master_volume(1.0);
        }
        assert_eq!(queue.drain().len(), QUEUE_LIMIT);
        assert_eq!(queue.requested(), 2);
    }
}
