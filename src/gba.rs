use super::arm_core;
use super::sync;
use super::timing;

pub const SCREEN_WIDTH: u32 = mgba_sys::GBA_VIDEO_HORIZONTAL_PIXELS as u32;
pub const SCREEN_HEIGHT: u32 = mgba_sys::GBA_VIDEO_VERTICAL_PIXELS as u32;

/// The GBA board, viewed in place: a `#[repr(transparent)]` wrapper over
/// the C struct, only ever handed out as `&Gba` / `&mut Gba` borrowed
/// from a [`Core`](crate::core::Core).
#[repr(transparent)]
pub struct Gba(pub(super) mgba_sys::GBA);

impl Gba {
    pub fn cpu(&self) -> &arm_core::ArmCore {
        unsafe { &*(self.0.cpu as *const arm_core::ArmCore) }
    }

    pub fn cpu_mut(&mut self) -> &mut arm_core::ArmCore {
        unsafe { &mut *(self.0.cpu as *mut arm_core::ArmCore) }
    }

    pub fn timing(&self) -> &timing::Timing {
        unsafe { &*(&self.0.timing as *const _ as *const timing::Timing) }
    }

    pub fn master_volume(&self) -> i32 {
        self.0.audio.masterVolume
    }

    pub fn set_master_volume(&mut self, volume: i32) {
        self.0.audio.masterVolume = volume;
    }

    /// Set the video frameskip. A large value makes the GBA video module skip
    /// `drawScanline` + `finishFrame` outright (both gated on
    /// `frameskipCounter <= 0` in gba/video.c), so the core advances game logic
    /// without rasterizing. Used on the headless fast-forward and shadow cores,
    /// whose pixels are never shown — the display core re-renders from the save
    /// states they capture, and VRAM/IO are driven by the CPU, not the renderer.
    /// `frameskip` isn't part of the serialized state, so loading a save state
    /// (e.g. one captured on a rendering core) won't clear it.
    pub fn set_frameskip(&mut self, frameskip: i32) {
        self.0.video.frameskip = frameskip;
        self.0.video.frameskipCounter = frameskip;
    }

    pub fn sync(&self) -> Option<&sync::Sync> {
        let sync_ptr = self.0.sync;
        if sync_ptr.is_null() {
            None
        } else {
            Some(unsafe { &*(sync_ptr as *const sync::Sync) })
        }
    }

    pub fn sync_mut(&mut self) -> Option<&mut sync::Sync> {
        let sync_ptr = self.0.sync;
        if sync_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *(sync_ptr as *mut sync::Sync) })
        }
    }

    /// The raw C struct, for state surgery the bindings don't cover
    /// (mgba-siolink's snapshot side-channels).
    pub fn as_raw(&mut self) -> *mut mgba_sys::GBA {
        &mut self.0
    }
}
