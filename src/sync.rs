/// A core's `mCoreSync`, viewed in place: a `#[repr(transparent)]`
/// wrapper only ever handed out as `&Sync` / `&mut Sync` borrowed from a
/// [`Gba`](crate::gba::Gba).
#[repr(transparent)]
pub struct Sync(pub(super) mgba_sys::mCoreSync);

impl Sync {
    pub fn fps_target(&self) -> f32 {
        self.0.fpsTarget
    }

    pub fn set_fps_target(&mut self, fps_target: f32) {
        self.0.fpsTarget = fps_target;
    }

    // mCoreSyncLoadCoreOpts pins audioHighWater at 512 frames, sized for the
    // 32 kHz default source rate. Battle Network games (and any title that
    // bumps SOUNDBIAS.resolution) push the GBA audio rate up to 65/131/262
    // kHz, at which point 512 source frames isn't enough to fill a single
    // host audio callback — the producer blocks every fill and the emulator
    // throttles below realtime (audible as low-pitched playback + underrun
    // crunch). Tango rescales this per fill mirroring mGBA's SDL frontend.
    pub fn set_audio_high_water(&mut self, frames: u32) {
        self.0.audioHighWater = frames as _;
    }
}
