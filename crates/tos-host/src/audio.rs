//! TempleOS music parsing and host tone output.

use std::ffi::CStr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const NOTE_MAP: [i64; 7] = [0, 2, 3, 5, 7, 8, 10];
const TONE_SLICE: Duration = Duration::from_millis(40);

static CURRENT_ONA: AtomicI64 = AtomicI64::new(0);
static TONE_WORKER: OnceLock<()> = OnceLock::new();
static MUSIC: OnceLock<Mutex<MusicState>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq)]
struct MusicState {
    octave: i64,
    note_len: f64,
    meter_top: i64,
    meter_bottom: i64,
    tempo: f64,
    staccato_factor: f64,
}

impl Default for MusicState {
    fn default() -> Self {
        Self {
            octave: 4,
            note_len: 1.0,
            meter_top: 4,
            meter_bottom: 4,
            tempo: 2.5,
            staccato_factor: 0.9,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct NoteEvent {
    ona: i64,
    on_seconds: f64,
    off_seconds: f64,
}

fn music() -> &'static Mutex<MusicState> {
    MUSIC.get_or_init(|| Mutex::new(MusicState::default()))
}

fn note_to_ona(note: i64, octave: i64) -> i64 {
    if note < 3 {
        (octave + 1) * 12 + note
    } else {
        octave * 12 + note
    }
}

fn parse_song(song: &[u8], state: &mut MusicState) -> Vec<NoteEvent> {
    let mut events = Vec::new();
    let mut cursor = 0;

    while cursor < song.len() {
        let mut tie = false;
        loop {
            let previous = cursor;
            if song.get(cursor) == Some(&b'(') {
                tie = true;
                cursor += 1;
            } else {
                while song.get(cursor) == Some(&b'M') {
                    cursor += 1;
                    if let Some(ch @ b'0'..=b'9') = song.get(cursor).copied() {
                        state.meter_top = i64::from(ch - b'0');
                        cursor += 1;
                    }
                    if song.get(cursor) == Some(&b'/') {
                        cursor += 1;
                    }
                    if let Some(ch @ b'0'..=b'9') = song.get(cursor).copied() {
                        state.meter_bottom = i64::from(ch - b'0');
                        cursor += 1;
                    }
                }
                while let Some(ch @ b'0'..=b'9') = song.get(cursor).copied() {
                    state.octave = i64::from(ch - b'0');
                    cursor += 1;
                }
                while let Some(ch) = song.get(cursor).copied() {
                    match ch {
                        b'w' => state.note_len = 4.0,
                        b'h' => state.note_len = 2.0,
                        b'q' => state.note_len = 1.0,
                        b'e' => state.note_len = 0.5,
                        b's' => state.note_len = 0.25,
                        b't' => state.note_len *= 2.0 / 3.0,
                        b'.' => state.note_len *= 1.5,
                        _ => break,
                    }
                    cursor += 1;
                }
            }
            if cursor == previous {
                break;
            }
        }

        let Some(ch) = song.get(cursor).copied() else {
            break;
        };
        cursor += 1;

        let note_index = i64::from(ch) - i64::from(b'A');
        let ona = if (0..7).contains(&note_index) {
            let mut note = NOTE_MAP[note_index as usize];
            let mut octave = state.octave;
            match song.get(cursor).copied() {
                Some(b'b') => {
                    note -= 1;
                    if note == 2 {
                        octave -= 1;
                    }
                    cursor += 1;
                }
                Some(b'#') => {
                    note += 1;
                    if note == 3 {
                        octave += 1;
                    }
                    cursor += 1;
                }
                _ => {}
            }
            note_to_ona(note, octave)
        } else {
            0
        };

        let duration = if state.tempo > 0.0 {
            state.note_len / state.tempo
        } else {
            0.0
        };
        let (on_seconds, off_seconds) = if tie {
            (duration, 0.0)
        } else {
            (
                duration * state.staccato_factor,
                duration * (1.0 - state.staccato_factor),
            )
        };
        events.push(NoteEvent {
            ona,
            on_seconds,
            off_seconds,
        });
    }

    events
}

fn ona_frequency(ona: i64) -> Option<u32> {
    if ona <= 0 {
        return None;
    }
    let frequency = 440.0 / 32.0 * 2.0_f64.powf(ona as f64 / 12.0);
    Some(frequency.round().clamp(37.0, 32_767.0) as u32)
}

fn start_tone_worker() {
    TONE_WORKER.get_or_init(|| {
        let _ = std::thread::Builder::new()
            .name("sanctum-audio".into())
            .spawn(|| {
                loop {
                    let ona = CURRENT_ONA.load(Ordering::Acquire);
                    if let Some(frequency) = ona_frequency(ona) {
                        play_tone_slice(frequency, TONE_SLICE);
                    } else {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            });
    });
}

fn set_note(ona: i64) {
    if ona > 0 {
        start_tone_worker();
    }
    CURRENT_ONA.store(ona.max(0), Ordering::Release);
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "Beep"]
    fn windows_beep(frequency: u32, duration_ms: u32) -> i32;
}

fn play_tone_slice(frequency: u32, duration: Duration) {
    let started = Instant::now();
    #[cfg(windows)]
    unsafe {
        let _ = windows_beep(frequency, duration.as_millis() as u32);
    }
    let remaining = duration.saturating_sub(started.elapsed());
    if !remaining.is_zero() {
        std::thread::sleep(remaining);
    }
}

fn sleep(duration: Duration) {
    tos_runtime::background_checkpoint();
    std::thread::sleep(duration);
}

pub fn reset() {
    stop();
    *music().lock().expect("music state poisoned") = MusicState::default();
}

pub fn stop() {
    set_note(0);
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_SndTaskEndCB() {
    set_note(0);
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Snd(ona: i64) {
    set_note(ona);
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_Beep(ona: i64, _busy: i64) {
    set_note(ona);
    sleep(Duration::from_millis(500));
    set_note(0);
    sleep(Duration::from_millis(200));
}

#[unsafe(no_mangle)]
/// Play a TempleOS music string using persistent global music settings.
///
/// # Safety
///
/// `song` must be null or point to a readable NUL-terminated byte string.
pub unsafe extern "C" fn tos_Play(song: *const u8, _words: *const u8) {
    tos_runtime::background_checkpoint();
    if song.is_null() {
        return;
    }
    let song = unsafe { CStr::from_ptr(song.cast()) }.to_bytes();
    let events = parse_song(song, &mut music().lock().expect("music state poisoned"));
    for event in events {
        set_note(event.ona);
        sleep(Duration::from_secs_f64(event.on_seconds));
        set_note(0);
        sleep(Duration::from_secs_f64(event.off_seconds));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tos_MusicSettingsRst() {
    reset();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_templeos_ona_to_frequency() {
        assert_eq!(ona_frequency(0), None);
        assert_eq!(ona_frequency(60), Some(440));
        assert_eq!(ona_frequency(72), Some(880));
    }

    #[test]
    fn parses_talons_octave_and_eighth_note_prefix() {
        let mut state = MusicState::default();
        let events = parse_song(b"5eCG", &mut state);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].ona, 63);
        assert_eq!(events[1].ona, 70);
        assert!((events[0].on_seconds - 0.18).abs() < 1e-9);
        assert!((events[0].off_seconds - 0.02).abs() < 1e-9);
    }

    #[test]
    fn preserves_music_settings_between_play_calls() {
        let mut state = MusicState::default();
        let first = parse_song(b"6qA", &mut state);
        let second = parse_song(b"B", &mut state);
        assert_eq!(first[0].ona, 84);
        assert_eq!(second[0].ona, 86);
        assert!((second[0].on_seconds - 0.36).abs() < 1e-9);
    }

    #[test]
    fn supports_ties_dots_and_accidentals() {
        let mut state = MusicState::default();
        let events = parse_song(b"(q.A#Cb", &mut state);
        assert_eq!(events[0].ona, 61);
        assert!((events[0].on_seconds - 0.6).abs() < 1e-9);
        assert_eq!(events[0].off_seconds, 0.0);
        assert_eq!(events[1].ona, 50);
    }
}
