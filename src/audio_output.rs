//! Plays [`AudioCommand`]s with kira: through the system audio device in a
//! window, or through an offline mixer that scenarios render tick by tick.

use std::path::Path;
use std::time::Duration;

use crate::runtime::AudioCommand;

/// Sample rate of the offline mix.
pub const MIX_RATE: u32 = 48_000;

/// One sound that has not ended, as scenarios see it under `audio:/playing`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PlayingSound {
    pub id: u64,
    pub clip: String,
    /// The volume it was asked for, or the target of its last fade.
    pub volume: f32,
    pub pan: f32,
    pub bus: String,
    pub looped: bool,
    /// Fixed tick it starts on.
    pub tick: u64,
}

#[cfg(feature = "audio")]
pub struct AudioOutput<B: kira::backend::Backend = kira::DefaultBackend> {
    manager: kira::AudioManager<B>,
    clips: std::collections::HashMap<
        std::path::PathBuf,
        kira::sound::static_sound::StaticSoundData,
    >,
    voices: std::collections::BTreeMap<
        u64,
        (kira::sound::static_sound::StaticSoundHandle, PlayingSound),
    >,
    buses: std::collections::HashMap<String, kira::track::TrackHandle>,
}

#[cfg(feature = "audio")]
impl AudioOutput {
    /// `None` when there is no audio device; the game runs silent.
    pub fn open() -> Option<Self> {
        AudioOutput::with_backend(Default::default())
            .inspect_err(|error| {
                eprintln!("audio: no output device, running silent: {error}");
            })
            .ok()
    }
}

/// A kira backend with no device: [`AudioOutput::render`] pulls samples.
#[cfg(feature = "audio")]
pub struct OfflineBackend(Option<kira::backend::Renderer>);

#[cfg(feature = "audio")]
impl kira::backend::Backend for OfflineBackend {
    type Settings = ();
    type Error = std::convert::Infallible;

    fn setup(
        _: (),
        _internal_buffer_size: usize,
    ) -> Result<(Self, u32), Self::Error> {
        Ok((Self(None), MIX_RATE))
    }

    fn start(
        &mut self,
        renderer: kira::backend::Renderer,
    ) -> Result<(), Self::Error> {
        self.0 = Some(renderer);
        Ok(())
    }
}

/// The mixer scenarios render into.
#[cfg(feature = "audio")]
pub type OfflineMixer = AudioOutput<OfflineBackend>;
#[cfg(not(feature = "audio"))]
pub type OfflineMixer = AudioOutput;

#[cfg(feature = "audio")]
impl AudioOutput<OfflineBackend> {
    pub fn offline() -> Option<Self> {
        Self::with_backend(()).ok()
    }

    /// Mixes the next `frames` stereo frames, as `[left, right, ...]`.
    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        if let Some(renderer) = &mut self.manager.backend_mut().0 {
            renderer.on_start_processing();
            renderer.process(&mut out, 2);
        }
        out
    }
}

#[cfg(feature = "audio")]
impl<B: kira::backend::Backend> AudioOutput<B>
where
    B::Settings: Default,
{
    fn with_backend(backend_settings: B::Settings) -> Result<Self, B::Error> {
        let manager = kira::AudioManager::new(kira::AudioManagerSettings {
            backend_settings,
            ..Default::default()
        })?;
        Ok(Self {
            manager,
            clips: Default::default(),
            voices: Default::default(),
            buses: Default::default(),
        })
    }

    /// Runs one command. `delay` gives how long from now a sound for a
    /// fixed tick should start.
    pub fn run(
        &mut self,
        assets: &Path,
        command: AudioCommand,
        delay: impl Fn(u64) -> Duration,
    ) {
        use kira::sound::static_sound::StaticSoundData;
        match command {
            AudioCommand::Play {
                id,
                clip,
                volume,
                looped,
                pan,
                bus,
                tick,
            } => {
                let path = assets.join(&clip);
                if !self.clips.contains_key(&path) {
                    match StaticSoundData::from_file(&path) {
                        Ok(data) => {
                            self.clips.insert(path.clone(), data);
                        }
                        Err(error) => {
                            eprintln!("audio: cannot play `{clip}`: {error}");
                            return;
                        }
                    }
                }
                let mut data = self.clips[&path]
                    .volume(decibels(volume))
                    .panning(pan)
                    .start_time(kira::StartTime::Delayed(delay(tick)));
                if looped {
                    data = data.loop_region(..);
                }
                let played = if bus.is_empty() {
                    self.manager.play(data)
                } else {
                    match self.bus(&bus) {
                        Some(track) => track.play(data),
                        None => return,
                    }
                };
                match played {
                    Ok(handle) => {
                        let info = PlayingSound {
                            id: id.0,
                            clip,
                            volume,
                            pan,
                            bus,
                            looped,
                            tick,
                        };
                        self.voices.insert(id.0, (handle, info));
                    }
                    Err(error) => {
                        eprintln!("audio: cannot play `{clip}`: {error}")
                    }
                }
            }
            AudioCommand::Stop(id) => {
                if let Some((mut handle, _)) = self.voices.remove(&id.0) {
                    handle.stop(kira::Tween::default());
                }
            }
            AudioCommand::StopAll => {
                for (_, (mut handle, _)) in std::mem::take(&mut self.voices) {
                    handle.stop(kira::Tween::default());
                }
            }
            AudioCommand::SetMasterVolume(volume) => {
                self.manager
                    .main_track()
                    .set_volume(decibels(volume), kira::Tween::default());
            }
            AudioCommand::SetVolume { id, volume, fade } => {
                if let Some((handle, info)) = self.voices.get_mut(&id.0) {
                    handle.set_volume(decibels(volume), tween(fade));
                    info.volume = volume;
                }
            }
            AudioCommand::SetBusVolume { bus, volume, fade } => {
                if let Some(track) = self.bus(&bus) {
                    track.set_volume(decibels(volume), tween(fade));
                }
            }
        }
    }

    /// Sounds that have not ended, in the order they were asked for.
    pub fn playing(&mut self) -> Vec<PlayingSound> {
        self.voices.retain(|_, (handle, _)| {
            handle.state() != kira::sound::PlaybackState::Stopped
        });
        self.voices.values().map(|(_, info)| info.clone()).collect()
    }

    fn bus(&mut self, name: &str) -> Option<&mut kira::track::TrackHandle> {
        if !self.buses.contains_key(name) {
            match self.manager.add_sub_track(Default::default()) {
                Ok(track) => {
                    self.buses.insert(name.to_owned(), track);
                }
                Err(error) => {
                    eprintln!("audio: cannot add bus `{name}`: {error}");
                    return None;
                }
            }
        }
        self.buses.get_mut(name)
    }
}

#[cfg(feature = "audio")]
fn tween(fade: f32) -> kira::Tween {
    kira::Tween {
        duration: Duration::from_secs_f32(fade.max(0.0)),
        ..Default::default()
    }
}

/// Linear gain as kira decibels; zero or less is silence.
#[cfg(feature = "audio")]
fn decibels(gain: f32) -> kira::Decibels {
    if gain <= 0.0 {
        kira::Decibels::SILENCE
    } else {
        kira::Decibels(20.0 * gain.log10())
    }
}

#[cfg(not(feature = "audio"))]
pub struct AudioOutput;

#[cfg(not(feature = "audio"))]
impl AudioOutput {
    pub fn open() -> Option<Self> {
        None
    }

    pub fn offline() -> Option<Self> {
        None
    }

    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        vec![0.0; frames * 2]
    }

    pub fn run(
        &mut self,
        _assets: &Path,
        _command: AudioCommand,
        _delay: impl Fn(u64) -> Duration,
    ) {
    }

    pub fn playing(&mut self) -> Vec<PlayingSound> {
        Vec::new()
    }
}

/// Writes interleaved stereo samples as a 16-bit WAV file.
pub fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&2u16.to_le_bytes()); // channels
    bytes.extend_from_slice(&MIX_RATE.to_le_bytes());
    bytes.extend_from_slice(&(MIX_RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}
