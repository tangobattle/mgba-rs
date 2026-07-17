use super::audio;
use super::gba;
use super::state;
use super::trapper;
use super::vfile;
use std::ffi::CString;
use std::mem::MaybeUninit;

/// The machine: a `#[repr(transparent)]` view over `mCore`, only ever
/// used behind `&Core` / `&mut Core` — the String/str split. Owning
/// handles are [`OwnedCore`] (which derefs here); cores that belong to C
/// (the core a trap handler runs against) are borrowed directly via
/// [`Core::from_raw_mut`]. Owner-only concerns (creation, teardown, trap
/// installation, the host video buffer) live on [`OwnedCore`] and are
/// statically unavailable through a borrow.
#[repr(transparent)]
pub struct Core(mgba_sys::mCore);

unsafe impl Send for Core {}

pub struct Options {
    pub sample_rate: u32,
    pub video_sync: bool,
    pub audio_sync: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            video_sync: false,
            audio_sync: false,
        }
    }
}

fn cstring_to_string(buf: &[std::os::raw::c_char]) -> String {
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

impl Core {
    /// Borrow a core that lives on the C side.
    ///
    /// # Safety
    ///
    /// `ptr` must point to a live, initialized `mCore` that nothing else
    /// aliases for the lifetime of the returned borrow.
    pub unsafe fn from_raw_mut<'a>(ptr: *mut mgba_sys::mCore) -> &'a mut Core {
        &mut *(ptr as *mut Core)
    }

    // The C vtable takes `mCore*` even for logically-const calls; the
    // cast never lets C outlive the borrow it came from.
    fn as_ptr(&self) -> *mut mgba_sys::mCore {
        &self.0 as *const _ as *mut _
    }

    pub fn gba(&self) -> &gba::Gba {
        unsafe { &*(self.0.board as *const gba::Gba) }
    }

    pub fn gba_mut(&mut self) -> &mut gba::Gba {
        unsafe { &mut *(self.0.board as *mut gba::Gba) }
    }

    pub fn frequency(&self) -> i32 {
        unsafe { (*self.as_ptr()).frequency.unwrap()(self.as_ptr()) }
    }

    fn game_info(&self) -> mgba_sys::mGameInfo {
        unsafe {
            let mut info = std::mem::zeroed::<mgba_sys::mGameInfo>();
            (*self.as_ptr()).getGameInfo.unwrap()(self.as_ptr(), &mut info);
            info
        }
    }

    pub fn game_title(&self) -> String {
        let info = self.game_info();
        cstring_to_string(&info.title)
    }

    pub fn game_code(&self) -> String {
        let info = self.game_info();
        cstring_to_string(&info.code)
    }

    pub fn crc32(&self) -> u32 {
        let mut c: u32 = 0;
        unsafe {
            (*self.as_ptr()).checksum.unwrap()(
                self.as_ptr(),
                &mut c as *mut _ as *mut std::ffi::c_void,
                mgba_sys::mCoreChecksumType_mCHECKSUM_CRC32,
            )
        };
        c
    }

    pub fn frame_counter(&self) -> u32 {
        unsafe { (*self.as_ptr()).frameCounter.unwrap()(self.as_ptr()) }
    }

    pub fn calculate_framerate_ratio(&self, desired_frame_rate: f64) -> f64 {
        unsafe { mgba_sys::mCoreCalculateFramerateRatio(self.as_ptr(), desired_frame_rate) }
    }

    pub fn audio_sample_rate(&self) -> u32 {
        unsafe { (*self.as_ptr()).audioSampleRate.unwrap()(self.as_ptr()) }
    }

    pub fn load_rom(&mut self, vf: vfile::VFile) -> Result<(), crate::Error> {
        if !unsafe { (*self.as_ptr()).loadROM.unwrap()(self.as_ptr(), vf.into_raw()) } {
            return Err(crate::Error::CallFailed("mCore.loadROM"));
        }
        Ok(())
    }

    pub fn load_save(&mut self, vf: vfile::VFile) -> Result<(), crate::Error> {
        if !unsafe { (*self.as_ptr()).loadSave.unwrap()(self.as_ptr(), vf.into_raw()) } {
            return Err(crate::Error::CallFailed("mCore.loadSave"));
        }
        Ok(())
    }

    pub fn load_state(&mut self, state: &state::State) -> Result<(), crate::Error> {
        if !unsafe { (*self.as_ptr()).loadState.unwrap()(self.as_ptr(), state.as_ptr() as *const _) } {
            return Err(crate::Error::CallFailed("mCore.loadState"));
        }
        Ok(())
    }

    pub fn save_state(&self) -> Result<Box<state::State>, crate::Error> {
        self.save_state_reusing(state::State::new_uninit())
    }

    /// [`save_state`](Self::save_state), but writing into a caller-provided
    /// buffer (typically one recycled via [`state::State::into_uninit`])
    /// instead of allocating a fresh ~400KB one.
    pub fn save_state_reusing(
        &self,
        mut buf: Box<MaybeUninit<state::State>>,
    ) -> Result<Box<state::State>, crate::Error> {
        self.save_state_into(buf.as_mut())?;
        Ok(unsafe { buf.assume_init() })
    }

    pub fn save_state_into<'b>(
        &self,
        state: &'b mut MaybeUninit<state::State>,
    ) -> Result<&'b mut state::State, crate::Error> {
        if !unsafe { (*self.as_ptr()).saveState.unwrap()(self.as_ptr(), state.as_mut_ptr() as *mut _) } {
            return Err(crate::Error::CallFailed("mCore.saveState"));
        }
        Ok(unsafe { state.assume_init_mut() })
    }

    pub fn set_keys(&mut self, keys: u32) {
        unsafe { (*self.as_ptr()).setKeys.unwrap()(self.as_ptr(), keys) }
    }

    pub fn raw_read_8(&self, address: u32, segment: i32) -> u8 {
        unsafe { (*self.as_ptr()).rawRead8.unwrap()(self.as_ptr(), address, segment) as u8 }
    }

    pub fn raw_read_16(&self, address: u32, segment: i32) -> u16 {
        unsafe { (*self.as_ptr()).rawRead16.unwrap()(self.as_ptr(), address, segment) as u16 }
    }

    pub fn raw_read_32(&self, address: u32, segment: i32) -> u32 {
        unsafe { (*self.as_ptr()).rawRead32.unwrap()(self.as_ptr(), address, segment) }
    }

    pub fn raw_read_range(&self, address: u32, segment: i32, buf: &mut [u8]) {
        for (i, v) in buf.iter_mut().enumerate() {
            *v = self.raw_read_8(address + i as u32, segment);
        }
    }

    pub fn raw_write_8(&mut self, address: u32, segment: i32, v: u8) {
        unsafe { (*self.as_ptr()).rawWrite8.unwrap()(self.as_ptr(), address, segment, v) }
    }

    pub fn raw_write_16(&mut self, address: u32, segment: i32, v: u16) {
        unsafe { (*self.as_ptr()).rawWrite16.unwrap()(self.as_ptr(), address, segment, v) }
    }

    pub fn raw_write_32(&mut self, address: u32, segment: i32, v: u32) {
        unsafe { (*self.as_ptr()).rawWrite32.unwrap()(self.as_ptr(), address, segment, v) }
    }

    pub fn raw_write_range(&mut self, address: u32, segment: i32, buf: &[u8]) {
        for (i, v) in buf.iter().enumerate() {
            self.raw_write_8(address + i as u32, segment, *v);
        }
    }

    pub fn run_frame(&mut self) {
        unsafe { (*self.as_ptr()).runFrame.unwrap()(self.as_ptr()) }
    }

    pub fn run_loop(&mut self) {
        unsafe { (*self.as_ptr()).runLoop.unwrap()(self.as_ptr()) }
    }

    pub fn end_run_loop(&mut self) {
        unsafe {
            let cpu = (*(self.0.board as *mut mgba_sys::GBA)).cpu;
            (*cpu).nextEvent = (*cpu).cycles;
        }
    }

    pub fn step(&mut self) {
        unsafe { (*self.as_ptr()).step.unwrap()(self.as_ptr()) }
    }

    pub fn reset(&mut self) {
        unsafe { (*self.as_ptr()).reset.unwrap()(self.as_ptr()) }
    }

    pub fn audio_buffer_size(&self) -> u64 {
        unsafe { (*self.as_ptr()).getAudioBufferSize.unwrap()(self.as_ptr()) as _ }
    }

    pub fn set_audio_buffer_size(&mut self, size: u64) {
        unsafe { (*self.as_ptr()).setAudioBufferSize.unwrap()(self.as_ptr(), size as _) }
    }

    pub fn audio_buffer(&mut self) -> &mut audio::AudioBuffer {
        unsafe { &mut *((*self.as_ptr()).getAudioBuffer.unwrap()(self.as_ptr()) as *mut audio::AudioBuffer) }
    }
}

/// A core this crate created and will deinitialize, plus the state only
/// an owner has: the host-side video buffer, installed traps, and a
/// pinned RTC override. Derefs to [`Core`] for the machine itself.
pub struct OwnedCore {
    ptr: *mut mgba_sys::mCore,
    video_buffer: Option<Vec<u8>>,
    trapper: Option<trapper::Trapper>,
    // Box so the heap location stays stable across OwnedCore moves; mgba's
    // mCoreSetRTC stores a `*mut mRTCSource` that points inside this
    // allocation and dereferences it on every cart RTC read.
    rtc: Option<Box<mgba_sys::mRTCGenericSource>>,
}

unsafe impl Send for OwnedCore {}

impl OwnedCore {
    pub fn new_gba(config_name: &str, options: &Options) -> Result<Self, crate::Error> {
        let ptr = unsafe { mgba_sys::GBACoreCreate() };
        if ptr.is_null() {
            return Err(crate::Error::CallFailed("GBACoreCreate"));
        }
        unsafe {
            {
                // TODO: Make this more generic maybe.
                let opts = &mut ptr.as_mut().unwrap().opts;
                opts.sampleRate = options.sample_rate as _;
                opts.videoSync = options.video_sync;
                opts.audioSync = options.audio_sync;
            }

            (*ptr).init.unwrap()(ptr);
            let config_name_cstr = CString::new(config_name).unwrap();
            mgba_sys::mCoreConfigInit(&mut ptr.as_mut().unwrap().config, config_name_cstr.as_ptr());
            mgba_sys::mCoreConfigLoad(&mut ptr.as_mut().unwrap().config);
        }

        Ok(OwnedCore {
            ptr,
            video_buffer: None,
            trapper: None,
            rtc: None,
        })
    }

    /// Pin the cart's RTC chip to a fixed time instead of host wallclock.
    /// Required for byte-stable replay playback of games that store RTC
    /// reads into RAM (e.g. BN4.5 surfaces seconds into WRAM). Call
    /// before `reset()` -- the game reads RTC during boot. Times before
    /// the unix epoch clamp to the epoch (the cart RTC has nowhere
    /// sensible to represent them).
    pub fn set_rtc_fixed(&mut self, time: std::time::SystemTime) {
        let unix_seconds = time
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut rtc = Box::new(unsafe { std::mem::zeroed::<mgba_sys::mRTCGenericSource>() });
        unsafe {
            mgba_sys::mRTCGenericSourceInit(rtc.as_mut() as *mut _, self.ptr);
            rtc.override_ = mgba_sys::mRTCGenericType_RTC_FIXED;
            rtc.value = unix_seconds;
            mgba_sys::mCoreSetRTC(self.ptr, &mut rtc.d as *mut _);
        }
        self.rtc = Some(rtc);
    }

    pub fn enable_video_buffer(&mut self) {
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        unsafe { (*self.ptr).baseVideoSize.unwrap()(self.ptr, &mut width, &mut height) };

        // Sized and typed off mColor so this tracks the build's color depth:
        // 32-bit XBGR8 by default, or 16-bit BGR555 under -DCOLOR_16_BIT.
        let mut buffer = vec![0u8; (width * height) as usize * std::mem::size_of::<mgba_sys::mColor>()];
        unsafe {
            (*self.ptr).setVideoBuffer.unwrap()(self.ptr, buffer.as_mut_ptr() as *mut mgba_sys::mColor, width as _);
        }
        self.video_buffer = Some(buffer);
    }

    pub fn video_buffer(&self) -> Option<&[u8]> {
        self.video_buffer.as_deref()
    }

    /// Install instruction traps: each `(address, handler)` patches a
    /// Thumb BKPT over the instruction at `address` and calls `handler`
    /// (after running the displaced instruction) whenever it executes.
    ///
    /// This is deliberately the only way to install traps. The trapper is
    /// spliced into the CPU's component table, which the core dereferences
    /// right up through its own deinit, so it must live exactly as long as
    /// the core — owning it here makes that structural (`OwnedCore`'s drop
    /// runs deinit before fields drop).
    ///
    /// Panics if traps are already installed: there is no uninstall, so a
    /// replacement would leave the first set's ROM patches live with their
    /// handlers gone, and chain the bkpt16 hook into itself.
    pub fn set_traps(&mut self, traps: Vec<(u32, Box<dyn Fn(&mut Core)>)>) {
        assert!(self.trapper.is_none(), "traps are already installed on this core");
        self.trapper = Some(trapper::Trapper::new(self.ptr, traps));
    }
}

impl std::ops::Deref for OwnedCore {
    type Target = Core;

    fn deref(&self) -> &Core {
        unsafe { &*(self.ptr as *const Core) }
    }
}

impl std::ops::DerefMut for OwnedCore {
    fn deref_mut(&mut self) -> &mut Core {
        unsafe { &mut *(self.ptr as *mut Core) }
    }
}

impl Drop for OwnedCore {
    fn drop(&mut self) {
        unsafe {
            mgba_sys::mCoreConfigDeinit(&mut self.ptr.as_mut().unwrap().config);
            (*self.ptr).deinit.unwrap()(self.ptr)
        }
    }
}
