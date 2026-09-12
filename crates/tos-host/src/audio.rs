//! TempleOS music parsing and host tone output.

use std::ffi::CStr;
use std::mem::{offset_of, size_of};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const NOTE_MAP: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];
static CURRENT_ONA: AtomicI64 = AtomicI64::new(0);
static MUSIC_GLOBAL: AtomicUsize = AtomicUsize::new(0);
static AUDIO_OUTPUT: OnceLock<Result<AudioOutput, String>> = OnceLock::new();
static MUSIC: OnceLock<Mutex<MusicState>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq)]
struct MusicState {
    octave: i64,
    note_len: f64,
    meter_top: i64,
    meter_bottom: i64,
    tempo: f64,
    staccato_factor: f64,
    note_map: [u8; 7],
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
            note_map: NOTE_MAP,
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
            let mut note = i64::from(state.note_map[note_index as usize]);
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
    if let Err(error) = AUDIO_OUTPUT.get_or_init(AudioOutput::new) {
        eprintln!("Sanctum audio unavailable: {error}");
    }
}

fn set_note(ona: i64) {
    if ona > 0 {
        start_tone_worker();
    }
    CURRENT_ONA.store(ona.max(0), Ordering::Release);
}

fn bound_music() -> Option<*mut u8> {
    let address = MUSIC_GLOBAL.load(Ordering::Acquire);
    (address != 0).then_some(address as *mut u8)
}

unsafe fn read_i64(base: *mut u8, offset: usize) -> i64 {
    unsafe { base.add(offset).cast::<i64>().read_unaligned() }
}

unsafe fn read_f64(base: *mut u8, offset: usize) -> f64 {
    unsafe { base.add(offset).cast::<f64>().read_unaligned() }
}

unsafe fn write_i64(base: *mut u8, offset: usize, value: i64) {
    unsafe { base.add(offset).cast::<i64>().write_unaligned(value) };
}

unsafe fn write_f64(base: *mut u8, offset: usize, value: f64) {
    unsafe { base.add(offset).cast::<f64>().write_unaligned(value) };
}

fn sync_from_bound(state: &mut MusicState) {
    let Some(base) = bound_music() else {
        return;
    };
    unsafe { sync_from_address(state, base) };
}

unsafe fn sync_from_address(state: &mut MusicState, base: *mut u8) {
    unsafe {
        state.octave = read_i64(base, offset_of!(tos_abi::CMusicGlbls, octave));
        let note_len = read_f64(base, offset_of!(tos_abi::CMusicGlbls, note_len));
        if note_len.is_finite() && note_len > 0.0 {
            state.note_len = note_len;
        }
        for (index, note) in state.note_map.iter_mut().enumerate() {
            *note = base
                .add(offset_of!(tos_abi::CMusicGlbls, note_map) + index)
                .read();
        }
        state.meter_top = read_i64(base, offset_of!(tos_abi::CMusicGlbls, meter_top));
        state.meter_bottom = read_i64(base, offset_of!(tos_abi::CMusicGlbls, meter_bottom));
        let tempo = read_f64(base, offset_of!(tos_abi::CMusicGlbls, tempo));
        if tempo.is_finite() && tempo > 0.0 {
            state.tempo = tempo;
        }
        let staccato = read_f64(base, offset_of!(tos_abi::CMusicGlbls, staccato_factor));
        if staccato.is_finite() {
            state.staccato_factor = staccato.clamp(0.0, 1.0);
        }
    }
}

fn sync_to_bound(state: &MusicState, initialize: bool) {
    let Some(base) = bound_music() else {
        return;
    };
    unsafe { sync_to_address(state, base, initialize) };
}

unsafe fn sync_to_address(state: &MusicState, base: *mut u8, initialize: bool) {
    unsafe {
        write_i64(base, offset_of!(tos_abi::CMusicGlbls, octave), state.octave);
        write_f64(
            base,
            offset_of!(tos_abi::CMusicGlbls, note_len),
            state.note_len,
        );
        write_i64(
            base,
            offset_of!(tos_abi::CMusicGlbls, meter_top),
            state.meter_top,
        );
        write_i64(
            base,
            offset_of!(tos_abi::CMusicGlbls, meter_bottom),
            state.meter_bottom,
        );
        write_f64(base, offset_of!(tos_abi::CMusicGlbls, tempo), state.tempo);
        write_f64(
            base,
            offset_of!(tos_abi::CMusicGlbls, staccato_factor),
            state.staccato_factor,
        );
        write_i64(base, offset_of!(tos_abi::CMusicGlbls, play_note_num), 0);
        if initialize {
            for (index, note) in state.note_map.iter().copied().enumerate() {
                base.add(offset_of!(tos_abi::CMusicGlbls, note_map) + index)
                    .write(note);
            }
            write_i64(base, offset_of!(tos_abi::CMusicGlbls, mute), 0);
        }
    }
}

fn set_play_note_num(value: i64) {
    if let Some(base) = bound_music() {
        unsafe { write_i64(base, offset_of!(tos_abi::CMusicGlbls, play_note_num), value) };
    }
}

fn is_muted() -> bool {
    bound_music().is_some_and(|base| unsafe { is_muted_at(base) })
}

unsafe fn is_muted_at(base: *mut u8) -> bool {
    unsafe { read_i64(base, offset_of!(tos_abi::CMusicGlbls, mute)) != 0 }
}

pub fn bind_music_global(address: *mut u8, size: usize) {
    if address.is_null() || size < size_of::<tos_abi::CMusicGlbls>() {
        unbind();
        return;
    }
    MUSIC_GLOBAL.store(address as usize, Ordering::Release);
    let state = *music().lock().expect("music state poisoned");
    sync_to_bound(&state, true);
}

pub fn unbind() {
    MUSIC_GLOBAL.store(0, Ordering::Release);
}

struct AudioOutput {
    _stream: cpal::Stream,
}

impl AudioOutput {
    fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default output device")?;
        let supported = device
            .default_output_config()
            .map_err(|error| error.to_string())?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let channels = usize::from(config.channels);
        let sample_rate = config.sample_rate as f64;
        let mut phase = 0.0_f64;
        let error_callback = |error| eprintln!("Sanctum audio stream error: {error}");
        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config.clone(),
                move |data: &mut [f32], _| fill_audio(data, channels, sample_rate, &mut phase),
                error_callback,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                config.clone(),
                move |data: &mut [i16], _| fill_audio(data, channels, sample_rate, &mut phase),
                error_callback,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                config,
                move |data: &mut [u16], _| fill_audio(data, channels, sample_rate, &mut phase),
                error_callback,
                None,
            ),
            format => return Err(format!("unsupported output sample format {format:?}")),
        }
        .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self { _stream: stream })
    }
}

fn fill_audio<T: cpal::Sample + cpal::FromSample<f32>>(
    output: &mut [T],
    channels: usize,
    sample_rate: f64,
    phase: &mut f64,
) {
    let frequency = ona_frequency(CURRENT_ONA.load(Ordering::Acquire)).unwrap_or(0) as f64;
    for frame in output.chunks_mut(channels.max(1)) {
        let value = if frequency == 0.0 {
            0.0
        } else if *phase < 0.5 {
            0.18
        } else {
            -0.18
        };
        if frequency != 0.0 {
            *phase = (*phase + frequency / sample_rate) % 1.0;
        }
        for sample in frame {
            *sample = T::from_sample(value);
        }
    }
}

fn sleep(duration: Duration) {
    tos_runtime::background_checkpoint();
    std::thread::sleep(duration);
}

pub fn reset() {
    stop();
    let state = MusicState::default();
    *music().lock().expect("music state poisoned") = state;
    sync_to_bound(&state, false);
}

pub fn stop() {
    set_note(0);
}

pub fn set_muted(muted: bool) {
    if muted {
        set_note(0);
    }
    if let Some(base) = bound_music() {
        unsafe {
            write_i64(
                base,
                offset_of!(tos_abi::CMusicGlbls, mute),
                i64::from(muted),
            )
        };
    }
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
    let events = {
        let mut state = music().lock().expect("music state poisoned");
        sync_from_bound(&mut state);
        let events = parse_song(song, &mut state);
        sync_to_bound(&state, false);
        events
    };
    for (index, event) in events.into_iter().enumerate() {
        if !is_muted() {
            set_note(event.ona);
        }
        sleep(Duration::from_secs_f64(event.on_seconds));
        if !is_muted() {
            set_note(0);
        }
        sleep(Duration::from_secs_f64(event.off_seconds));
        set_play_note_num(index as i64 + 1);
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
    fn maps_canonical_music_settings_and_reads_mute() {
        let mut globals = [0_u8; size_of::<tos_abi::CMusicGlbls>()];
        let state = MusicState::default();
        unsafe {
            sync_to_address(&state, globals.as_mut_ptr(), true);
            assert_eq!(
                read_i64(
                    globals.as_mut_ptr(),
                    offset_of!(tos_abi::CMusicGlbls, octave)
                ),
                4
            );
            assert_eq!(
                read_f64(
                    globals.as_mut_ptr(),
                    offset_of!(tos_abi::CMusicGlbls, tempo)
                ),
                2.5
            );
            write_f64(
                globals.as_mut_ptr(),
                offset_of!(tos_abi::CMusicGlbls, tempo),
                5.0,
            );
            write_i64(
                globals.as_mut_ptr(),
                offset_of!(tos_abi::CMusicGlbls, mute),
                1,
            );
            let mut updated = state;
            sync_from_address(&mut updated, globals.as_mut_ptr());
            assert_eq!(updated.tempo, 5.0);
            assert!(is_muted_at(globals.as_mut_ptr()));
        }
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
