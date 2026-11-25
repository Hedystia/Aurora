//! Kira mixer buses and predecoded clip references.

use std::{collections::BTreeMap, io::Cursor};

use anyhow::{Context, Result};
use kira::{
    AudioManager, AudioManagerSettings, DefaultBackend,
    backend::mock::{MockBackend, MockBackendSettings},
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
    track::{TrackBuilder, TrackHandle},
};

/// A decoded sound clip safe to reuse across one-shot requests.
#[derive(Clone)]
pub struct AudioClip(StaticSoundData);

impl AudioClip {
    /// Decodes an audio file from bytes before playback.
    pub fn decode(bytes: Vec<u8>) -> Result<Self> {
        Ok(Self(StaticSoundData::from_cursor(Cursor::new(bytes))?))
    }

    /// Plays this clip through the SFX bus.
    pub fn play(&self, audio: &mut Audio) -> Result<StaticSoundHandle> {
        Ok(audio.effects.play(self.0.clone())?)
    }
}

/// An ordered collection of clips that scripts can reference by index.
pub struct AudioSource {
    clips: Vec<AudioClip>,
    dynamic: BTreeMap<u64, AudioClip>,
}

impl AudioSource {
    /// Creates a source with the specified clip ordering.
    pub fn new(clips: Vec<AudioClip>) -> Self {
        Self {
            clips,
            dynamic: BTreeMap::new(),
        }
    }

    /// Stores a disk-backed clip under a script handle.
    pub fn insert_dynamic(&mut self, handle: u64, clip: AudioClip) {
        self.dynamic.insert(handle, clip);
    }

    /// Drops a disk-backed clip. Already playing one-shots keep their clone.
    pub fn remove_dynamic(&mut self, handle: u64) {
        self.dynamic.remove(&handle);
    }

    /// Plays one clip through the SFX bus.
    pub fn play_one_shot(&self, audio: &mut Audio, index: usize) -> Result<StaticSoundHandle> {
        let clip = self
            .clips
            .get(index)
            .with_context(|| format!("Invalid audio clip index {index}"))?;
        Ok(audio.effects.play(clip.0.clone())?)
    }

    /// Plays a disk-backed clip through the same SFX bus.
    pub fn play_dynamic(&self, audio: &mut Audio, handle: u64) -> Result<StaticSoundHandle> {
        let clip = self
            .dynamic
            .get(&handle)
            .with_context(|| format!("Unknown audio handle {handle}"))?;
        Ok(audio.effects.play(clip.0.clone())?)
    }
}

enum Mixer {
    Device {
        _manager: Box<AudioManager<DefaultBackend>>,
    },
    Silent(Box<AudioManager<MockBackend>>),
}

/// Kira mixer with separate effects and music tracks.
pub struct Audio {
    mixer: Mixer,
    effects: TrackHandle,
    music: TrackHandle,
}

impl Audio {
    /// Opens the default audio device, falling back to the silent mixer if unavailable.
    pub fn new(headless: bool) -> Result<Self> {
        if !headless {
            match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
                Ok(mut manager) => {
                    let effects = manager.add_sub_track(TrackBuilder::default())?;
                    let music = manager.add_sub_track(TrackBuilder::default())?;
                    return Ok(Self {
                        mixer: Mixer::Device {
                            _manager: Box::new(manager),
                        },
                        effects,
                        music,
                    });
                }
                Err(error) => {
                    eprintln!("Audio device unavailable. Using the silent mixer. {error}")
                }
            }
        }
        let mut manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 48000 },
            internal_buffer_size: 800,
            ..Default::default()
        })
        .map_err(|()| anyhow::anyhow!("Creating silent mixer failed"))?;
        let effects = manager.add_sub_track(TrackBuilder::default())?;
        let music = manager.add_sub_track(TrackBuilder::default())?;
        Ok(Self {
            mixer: Mixer::Silent(Box::new(manager)),
            effects,
            music,
        })
    }

    /// Plays a clip on the music bus.
    pub fn play_music(&mut self, clip: &AudioClip) -> Result<StaticSoundHandle> {
        Ok(self.music.play(clip.0.clone())?)
    }

    /// Advances the silent destination by one simulation tick.
    pub fn step(&mut self) {
        if let Mixer::Silent(manager) = &mut self.mixer {
            manager.backend_mut().on_start_processing();
            manager.backend_mut().process();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::sound::PlaybackState;

    #[test]
    fn wav_one_shot_reaches_completion_on_the_effects_track() -> Result<()> {
        let clip = AudioClip::decode(include_bytes!("../app/demo/bird/flap.wav").to_vec())?;
        let mut mixer = Audio::new(true)?;
        let source = AudioSource::new(vec![clip]);
        assert!(source.play_one_shot(&mut mixer, 1).is_err());
        assert!(AudioClip::decode(b"invalid audio".to_vec()).is_err());
        let handle = source.play_one_shot(&mut mixer, 0)?;
        mixer.step();
        assert_eq!(handle.state(), PlaybackState::Playing);
        for _ in 0..12 {
            mixer.step();
        }
        assert_eq!(handle.state(), PlaybackState::Stopped);
        Ok(())
    }
}
