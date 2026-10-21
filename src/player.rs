use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player as RodioPlayer, Source};

use crate::playlist::Track;

pub const SILENCE_EPSILON: Duration = Duration::from_millis(250);
pub const DRIFT_TOLERANCE: Duration = Duration::from_millis(400);

#[derive(Debug, Default)]
struct Clock {
    accumulated: Duration,
    started: Option<Instant>,
}

impl Clock {
    fn now(&self) -> Duration {
        self.accumulated + self.started.map(|s| s.elapsed()).unwrap_or_default()
    }

    fn restart(&mut self) {
        self.accumulated = Duration::ZERO;
        self.started = Some(Instant::now());
    }

    fn pause(&mut self) {
        self.accumulated = self.now();
        self.started = None;
    }

    fn resume(&mut self) {
        self.started = Some(Instant::now());
    }

    fn reset(&mut self) {
        self.accumulated = Duration::ZERO;
        self.started = None;
    }

    fn force(&mut self, value: Duration) {
        self.accumulated = value;
        self.started = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped,
    Playing,
    Paused,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Playing => "playing",
            Self::Paused => "paused",
        }
    }
}

pub fn probe_duration(path: &Path) -> Option<Duration> {
    let file = File::open(path).ok()?;
    let decoder = Decoder::new(BufReader::new(file)).ok()?;
    let rate = f64::from(decoder.sample_rate().get());
    let samples = decoder.count() as u64;
    Some(Duration::from_secs_f64(samples as f64 / rate))
}

pub struct Player {
    rodio: Option<RodioPlayer>,
    _sink: Option<MixerDeviceSink>,
    pub silent: bool,
    state: State,
    volume: f32,
    duration: Duration,
    clock: Clock,
}

impl Player {
    pub fn new(volume: f32, force_silent: bool) -> Self {
        let mut this = Self {
            rodio: None,
            _sink: None,
            silent: true,
            state: State::Stopped,
            volume: volume.clamp(0.0, 1.0),
            duration: Duration::ZERO,
            clock: Clock::default(),
        };
        if force_silent {
            return this;
        }
        let opened =
            DeviceSinkBuilder::from_default_device().and_then(|b| b.open_sink_or_fallback());
        match opened {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                let player = RodioPlayer::connect_new(sink.mixer());
                player.set_volume(this.volume);
                this.rodio = Some(player);
                this._sink = Some(sink);
                this.silent = false;
            }
            Err(err) => {
                eprintln!("[!] 打开音频设备失败 ({err})，进入静音模式");
            }
        }
        this
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn set_volume(&mut self, value: f32) {
        self.volume = value.clamp(0.0, 1.0);
        if let Some(player) = &self.rodio {
            player.set_volume(self.volume);
        }
    }

    pub fn volume_percent(&self) -> u8 {
        (self.volume * 100.0).round() as u8
    }

    pub fn play(&mut self, track: &Track) -> Result<()> {
        let file = File::open(&track.path)
            .with_context(|| format!("打开音频文件失败: {}", track.path.display()))?;
        let duration = probe_duration(&track.path).unwrap_or_default();
        if let Some(player) = &self.rodio {
            let source = Decoder::new(BufReader::new(file))
                .with_context(|| format!("解码失败: {}", track.path.display()))?;
            player.clear();
            player.append(source);
            player.set_volume(self.volume);
            player.play();
        }
        self.duration = duration;
        self.clock.restart();
        self.state = State::Playing;
        Ok(())
    }

    pub fn toggle_pause(&mut self) {
        match self.state {
            State::Playing => {
                self.clock.pause();
                if let Some(player) = &self.rodio {
                    player.pause();
                }
                self.state = State::Paused;
            }
            State::Paused => {
                self.clock.resume();
                if let Some(player) = &self.rodio {
                    player.play();
                }
                self.state = State::Playing;
            }
            State::Stopped => {}
        }
    }

    pub fn stop(&mut self) {
        if let Some(player) = &self.rodio {
            player.clear();
            player.stop();
        }
        self.state = State::Stopped;
        self.clock.reset();
        self.duration = Duration::ZERO;
    }

    pub fn position(&self) -> Duration {
        self.clock.now()
    }

    pub fn tick(&mut self) {
        if self.state != State::Playing {
            return;
        }
        let Some(player) = &self.rodio else {
            return;
        };
        let device = player.get_pos();
        if device.abs_diff(self.clock.now()) > DRIFT_TOLERANCE {
            self.clock.force(device);
        }
    }

    pub fn ratio(&self) -> f32 {
        let total = self.duration.as_secs_f32();
        if total <= 0.0 {
            return 0.0;
        }
        (self.position().as_secs_f32() / total).clamp(0.0, 1.0)
    }

    pub fn finished(&self) -> bool {
        if self.state != State::Playing {
            return false;
        }
        if self.duration.is_zero() {
            return self.rodio.as_ref().is_some_and(|p| p.empty());
        }
        self.position() + SILENCE_EPSILON >= self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silent_wav(seconds: u32, rate: u32) -> Vec<u8> {
        let samples = rate * seconds;
        let data_len = samples * 2;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend(std::iter::repeat_n(0u8, data_len as usize));
        out
    }

    fn write_wav(name: &str, seconds: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("music_shell_test_player");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.wav"));
        std::fs::write(&path, silent_wav(seconds, 8_000)).unwrap();
        path
    }

    fn track_of(path: &std::path::Path) -> Track {
        Track::from_path(path.to_path_buf())
    }

    #[test]
    fn duration_is_derived_from_decoded_samples() {
        let path = write_wav("dur3", 3);
        let d = probe_duration(&path).unwrap();
        assert!((d.as_secs_f64() - 3.0).abs() < 0.05, "{d:?}");
    }

    #[test]
    fn unreadable_or_bogus_files_have_no_duration() {
        assert!(probe_duration(std::path::Path::new("/nope/none.wav")).is_none());
        let path = std::env::temp_dir().join("music_shell_test_player/bogus.mp3");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not audio at all").unwrap();
        assert!(probe_duration(&path).is_none());
    }

    #[test]
    fn silent_player_tracks_state_transitions() {
        let path = write_wav("state", 2);
        let mut p = Player::new(0.5, true);
        assert!(p.silent);
        assert_eq!(p.state(), State::Stopped);

        p.play(&track_of(&path)).unwrap();
        assert_eq!(p.state(), State::Playing);
        assert_eq!(p.duration().as_secs(), 2);

        p.toggle_pause();
        assert_eq!(p.state(), State::Paused);
        let frozen = p.position();
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(p.position(), frozen);

        p.toggle_pause();
        assert_eq!(p.state(), State::Playing);
        assert!(p.position() >= frozen);

        p.stop();
        assert_eq!(p.state(), State::Stopped);
        assert_eq!(p.position(), Duration::ZERO);
    }

    #[test]
    fn a_paused_player_never_reports_finished() {
        let path = write_wav("paused", 1);
        let mut p = Player::new(1.0, true);
        p.play(&track_of(&path)).unwrap();
        p.toggle_pause();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!p.finished());
    }

    #[test]
    fn a_stopped_player_never_reports_finished() {
        let path = write_wav("stopped", 1);
        let mut p = Player::new(1.0, true);
        p.play(&track_of(&path)).unwrap();
        p.stop();
        assert!(!p.finished());
    }

    #[test]
    fn volume_is_clamped_to_the_unit_range() {
        let mut p = Player::new(0.7, true);
        assert!((p.volume() - 0.7).abs() < 0.001);
        p.set_volume(5.0);
        assert_eq!(p.volume(), 1.0);
        assert_eq!(p.volume_percent(), 100);
        p.set_volume(-2.0);
        assert_eq!(p.volume(), 0.0);
        assert_eq!(p.volume_percent(), 0);
    }

    #[test]
    fn ratio_grows_with_position_and_clamps_to_one() {
        let path = write_wav("ratio", 4);
        let mut p = Player::new(1.0, true);
        p.play(&track_of(&path)).unwrap();
        assert!(p.ratio() < 0.01);
        std::thread::sleep(Duration::from_millis(150));
        let r = p.ratio();
        assert!(r > 0.0 && r < 0.2, "{r}");
        assert!(r <= 1.0);
    }

    #[test]
    fn a_finished_track_is_detected_once_its_duration_elapses() {
        let path = write_wav("short", 1);
        let mut p = Player::new(1.0, true);
        p.play(&track_of(&path)).unwrap();
        assert!(!p.finished());
        std::thread::sleep(Duration::from_millis(1_100));
        assert!(p.finished());
    }

    #[test]
    fn playing_a_missing_file_is_an_error_not_a_panic() {
        let mut p = Player::new(1.0, true);
        let missing = Track::from_path(std::path::PathBuf::from("/nope/none.mp3"));
        assert!(p.play(&missing).is_err());
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;

    fn wav(name: &str, seconds: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("music_shell_test_clock");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.wav"));
        let rate = 8_000u32;
        let samples = rate * seconds;
        let data_len = samples * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(out.len() + data_len as usize, 0);
        std::fs::write(&path, &out).unwrap();
        path
    }

    fn player_on(path: &std::path::Path) -> Player {
        let mut p = Player::new(1.0, true);
        p.play(&Track::from_path(path.to_path_buf())).unwrap();
        p
    }

    #[test]
    fn the_clock_only_advances_while_playing() {
        let path = wav("adv", 5);
        let mut p = player_on(&path);
        std::thread::sleep(Duration::from_millis(200));
        let a = p.position();
        assert!(a > Duration::ZERO, "播放时位置没推进");
        p.toggle_pause();
        let frozen = p.position();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(p.position(), frozen, "暂停后位置还在走");
        p.toggle_pause();
        std::thread::sleep(Duration::from_millis(200));
        assert!(p.position() > frozen, "继续后位置没接着走");
    }

    #[test]
    fn pausing_does_not_lose_elapsed_time() {
        let path = wav("keep", 5);
        let mut p = player_on(&path);
        std::thread::sleep(Duration::from_millis(300));
        let before = p.position();
        p.toggle_pause();
        assert!(p.position() >= before);
        assert!(p.position() < Duration::from_millis(600));
    }

    #[test]
    fn stop_resets_the_clock_entirely() {
        let path = wav("reset", 5);
        let mut p = player_on(&path);
        std::thread::sleep(Duration::from_millis(150));
        p.stop();
        assert_eq!(p.position(), Duration::ZERO);
        assert_eq!(p.duration(), Duration::ZERO);
    }

    #[test]
    fn switching_tracks_resets_the_clock() {
        let a = wav("a", 5);
        let b = wav("b", 5);
        let mut p = player_on(&a);
        std::thread::sleep(Duration::from_millis(200));
        p.play(&Track::from_path(b)).unwrap();
        assert!(
            p.position() < Duration::from_millis(100),
            "换歌后位置没归零"
        );
    }

    #[test]
    fn position_never_exceeds_the_duration() {
        let path = wav("clamp", 1);
        let p = player_on(&path);
        std::thread::sleep(Duration::from_millis(1200));
        assert!(p.position() <= p.duration() + Duration::from_millis(500));
        assert!(p.ratio() <= 1.0);
    }

    #[test]
    fn a_short_track_is_reported_as_finished() {
        let path = wav("tiny", 1);
        let p = player_on(&path);
        std::thread::sleep(Duration::from_millis(1100));
        assert!(p.finished());
    }

    #[test]
    fn tick_does_not_move_a_paused_clock() {
        let path = wav("tick", 5);
        let mut p = player_on(&path);
        p.toggle_pause();
        let before = p.position();
        p.tick();
        assert_eq!(p.position(), before);
    }
}
