//! A tiny synthesised typewriter.
//!
//! Every character that the typewriter effect puts on screen is accompanied by
//! a short burst of filtered noise, so it sounds as though someone in the back
//! room is tapping the keys for you. Nothing is loaded from disk: the clicks are
//! generated once at start-up and mixed straight into the audio device.

use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle};

const SAMPLE_RATE: u32 = 44_100;

/// How many slightly different clicks we pre-bake. Cycling through them keeps
/// the typing from sounding like a machine gun of identical samples.
const VARIANTS: usize = 8;

/// Overall loudness. Deliberately low; this is a hint, not a performance.
const GAIN: f32 = 0.16;

thread_local! {
    static TYPEWRITER: RefCell<Option<Typewriter>> = const { RefCell::new(None) };
}

/// Prepare the audio device and bake the click samples.
///
/// Safe to call when there is no sound card, no PipeWire, or when the user has
/// asked for silence with `BOXCLEVER_SILENT=1`; the typewriter simply stays mute.
///
/// Call this before the first screen is drawn: opening an ALSA device can print
/// warnings to stderr, and clearing the screen afterwards tidies them away.
pub fn init() {
    if std::env::var_os("BOXCLEVER_SILENT").is_some() {
        return;
    }
    let Some(typewriter) = Typewriter::open() else {
        return;
    };
    TYPEWRITER.with(|slot| *slot.borrow_mut() = Some(typewriter));
}

/// Play one keystroke for `ch`. Does nothing if audio is unavailable.
pub fn key_press(ch: char) {
    TYPEWRITER.with(|slot| {
        if let Some(typewriter) = slot.borrow().as_ref() {
            typewriter.strike(ch);
        }
    });
}

struct Typewriter {
    // Held only to keep the output device alive; dropping it stops all sound.
    _stream: OutputStream,
    handle: OutputStreamHandle,
    letters: Vec<Vec<f32>>,
    spaces: Vec<Vec<f32>>,
    rng: RefCell<Rng>,
}

impl Typewriter {
    fn open() -> Option<Self> {
        let (stream, handle) = OutputStream::try_default().ok()?;
        let mut rng = Rng::from_clock();

        // Letter keys: short, dry, a little scratchy.
        let letters = (0..VARIANTS)
            .map(|_| {
                let amplitude = rng.range(0.72, 1.0);
                let decay_ms = rng.range(3.4, 5.2);
                bake(&mut rng, 0.014, decay_ms, amplitude, 0.55)
            })
            .collect();

        // Space bar: a touch deeper and duller, the way a real one thuds.
        let spaces = (0..VARIANTS)
            .map(|_| {
                let amplitude = rng.range(0.80, 1.05);
                let decay_ms = rng.range(5.5, 7.5);
                bake(&mut rng, 0.020, decay_ms, amplitude, 0.80)
            })
            .collect();

        Some(Self {
            _stream: stream,
            handle,
            letters,
            spaces,
            rng: RefCell::new(rng),
        })
    }

    fn strike(&self, ch: char) {
        let bank = if ch == ' ' { &self.spaces } else { &self.letters };
        let index = self.rng.borrow_mut().below(bank.len());
        let samples = bank[index].clone();
        let _ = self
            .handle
            .play_raw(SamplesBuffer::new(1, SAMPLE_RATE, samples));
    }
}

/// Build one click.
///
/// White noise is differenced (a crude high-pass) to give it a papery scratch,
/// then rolled off again by a one-pole low-pass so it sounds like it is coming
/// through a wall rather than out of the speaker cone. An exponential envelope
/// collapses it into a keystroke.
///
/// * `seconds` — total length of the burst
/// * `decay_ms` — time constant of the envelope; smaller is snappier
/// * `amplitude` — per-variant loudness trim
/// * `muffle` — 0.0 is bright, 1.0 is heavily damped
fn bake(rng: &mut Rng, seconds: f32, decay_ms: f32, amplitude: f32, muffle: f32) -> Vec<f32> {
    let len = (SAMPLE_RATE as f32 * seconds) as usize;
    let mut samples = Vec::with_capacity(len);

    let mut previous_noise = 0.0f32;
    let mut lowpassed = 0.0f32;
    let smoothing = muffle.clamp(0.0, 0.95);

    for i in 0..len {
        let noise = rng.bipolar();
        let scratch = noise - previous_noise;
        previous_noise = noise;

        lowpassed = lowpassed * smoothing + scratch * (1.0 - smoothing);

        let t_ms = (i as f32 / SAMPLE_RATE as f32) * 1000.0;
        let envelope = (-t_ms / decay_ms).exp();

        samples.push(lowpassed * envelope * amplitude * GAIN);
    }

    // Ease the attack so the burst itself does not click.
    let fade = 8.min(samples.len());
    for (i, sample) in samples.iter_mut().take(fade).enumerate() {
        *sample *= i as f32 / fade as f32;
    }

    samples
}

/// A tiny xorshift generator. We only need noise and a bit of jitter, so
/// pulling in a full random-number crate would be overkill.
struct Rng(u32);

impl Rng {
    fn from_clock() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0x5EED_1234);
        Self(nanos | 1)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `0.0..1.0`.
    fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in `-1.0..1.0`.
    fn bipolar(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + self.unit() * (high - low)
    }

    fn below(&mut self, limit: usize) -> usize {
        if limit == 0 {
            0
        } else {
            (self.next_u32() as usize) % limit
        }
    }
}
