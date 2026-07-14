//! Bindings for mgba's SIO lockstep 2.0 stack (link-cable emulation).
//!
//! The C coordinator was designed to sync one `mCoreThread` per player, with
//! `mLockstepUser::sleep` blocking the calling thread until `wake`. These
//! bindings implement a *cooperative* user instead: `sleep`/`wake` only flip
//! a flag, and the C side already force-exits the sleeping core's run slice
//! (`cpu->nextEvent = 0` + `GBAInterrupt`). A frontend can therefore drive
//! every attached core from ONE thread by repeatedly calling `run_loop()` on
//! whichever cores are awake — which also makes the whole multi-core system
//! a deterministic function of its inputs, something the threaded mode does
//! not guarantee (secondaries observe the shared clock at thread-schedule-
//! dependent points). Determinism is the property rollback needs.
//!
//! Lifecycle contract (not enforced by the borrow checker):
//! - A `Driver` must outlive the core it is installed on, or be `uninstall`ed
//!   first: core deinit calls back into the driver.
//! - The `Coordinator` must outlive every `Driver` attached to it.
//! - `Driver::load_state` must be called after the owning core's
//!   `load_state` (the core load clears the timing list the driver blob
//!   re-schedules into), and only with a blob captured from the same
//!   attach configuration.

use super::core;

#[repr(C)]
struct UserGlue {
    // Must stay the first field: the C callbacks cast `*mut mLockstepUser`
    // back to `*mut UserGlue`.
    user: mgba_sys::mLockstepUser,
    asleep: std::sync::atomic::AtomicBool,
    requested_id: i32,
    player_id: std::sync::atomic::AtomicI32,
}

unsafe extern "C" fn c_sleep(user: *mut mgba_sys::mLockstepUser) {
    let glue = &*(user as *mut UserGlue);
    glue.asleep.store(true, std::sync::atomic::Ordering::Relaxed);
}

unsafe extern "C" fn c_wake(user: *mut mgba_sys::mLockstepUser) {
    let glue = &*(user as *mut UserGlue);
    glue.asleep.store(false, std::sync::atomic::Ordering::Relaxed);
}

unsafe extern "C" fn c_requested_id(user: *mut mgba_sys::mLockstepUser) -> std::os::raw::c_int {
    let glue = &*(user as *mut UserGlue);
    glue.requested_id as std::os::raw::c_int
}

unsafe extern "C" fn c_player_id_changed(user: *mut mgba_sys::mLockstepUser, id: std::os::raw::c_int) {
    let glue = &*(user as *mut UserGlue);
    glue.player_id.store(id as i32, std::sync::atomic::Ordering::Relaxed);
}

pub struct Coordinator {
    raw: Box<mgba_sys::GBASIOLockstepCoordinator>,
}

// The coordinator is only touched from whichever thread is currently driving
// the attached cores (all C entry points take its internal mutex anyway).
unsafe impl Send for Coordinator {}

impl Coordinator {
    pub fn new() -> Self {
        let mut raw = Box::new(unsafe { std::mem::zeroed::<mgba_sys::GBASIOLockstepCoordinator>() });
        unsafe {
            mgba_sys::GBASIOLockstepCoordinatorInit(&mut *raw);
        }
        Coordinator { raw }
    }

    /// Number of players currently registered (attached drivers whose cores
    /// have been reset at least once).
    pub fn attached_players(&mut self) -> usize {
        unsafe { mgba_sys::GBASIOLockstepCoordinatorAttached(&mut *self.raw) }
    }
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Coordinator {
    fn drop(&mut self) {
        unsafe {
            mgba_sys::GBASIOLockstepCoordinatorDeinit(&mut *self.raw);
        }
    }
}

pub struct Driver {
    raw: Box<mgba_sys::GBASIOLockstepDriver>,
    glue: Box<UserGlue>,
}

unsafe impl Send for Driver {}

impl Driver {
    /// `requested_id` is the player slot this core asks for when IDs are
    /// (re)assigned. Give every driver on a coordinator a distinct one so
    /// assignment never falls back to table-iteration order, which is not
    /// deterministic across processes.
    pub fn new(coordinator: &mut Coordinator, requested_id: i32) -> Self {
        let mut glue = Box::new(UserGlue {
            user: mgba_sys::mLockstepUser {
                sleep: Some(c_sleep),
                wake: Some(c_wake),
                requestedId: Some(c_requested_id),
                playerIdChanged: Some(c_player_id_changed),
            },
            asleep: std::sync::atomic::AtomicBool::new(false),
            requested_id,
            player_id: std::sync::atomic::AtomicI32::new(-1),
        });
        let mut raw = Box::new(unsafe { std::mem::zeroed::<mgba_sys::GBASIOLockstepDriver>() });
        unsafe {
            mgba_sys::GBASIOLockstepDriverCreate(&mut *raw, &mut glue.user);
            mgba_sys::GBASIOLockstepCoordinatorAttach(&mut *coordinator.raw, &mut *raw);
        }
        Driver { raw, glue }
    }

    /// Install this driver as the core's link port. The player is registered
    /// with the coordinator immediately (and re-registered on every core
    /// reset).
    pub fn install(&mut self, core: &mut core::CoreMutRef) {
        unsafe {
            mgba_sys::GBASIOSetDriver(&mut (*core.gba_mut().ptr).sio, &mut self.raw.d);
        }
    }

    /// Detach from the core's link port ahead of dropping this driver while
    /// the core lives on.
    pub fn uninstall(&mut self, core: &mut core::CoreMutRef) {
        unsafe {
            let sio = &mut (*core.gba_mut().ptr).sio;
            if sio.driver == &mut self.raw.d as *mut _ {
                mgba_sys::GBASIOSetDriver(sio, std::ptr::null_mut());
            }
        }
    }

    /// Whether the lockstep protocol has parked this core. A parked core's
    /// `run_loop` exits immediately; drive the other attached cores until
    /// they wake this one.
    pub fn asleep(&self) -> bool {
        self.glue.asleep.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The player ID assigned by the coordinator (0 = primary/clock owner),
    /// or -1 before the first assignment.
    pub fn player_id(&self) -> i32 {
        self.glue.player_id.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Serialize this player's lockstep state (for player 0, this includes
    /// the shared coordinator state). Core savestates do NOT cover any of
    /// this; snapshot both alongside each core's state.
    pub fn save_state(&mut self) -> Vec<u8> {
        let mut buf: *mut std::os::raw::c_void = std::ptr::null_mut();
        let mut size: usize = 0;
        unsafe {
            (self.raw.d.saveState.unwrap())(&mut self.raw.d, &mut buf, &mut size);
            let v = std::slice::from_raw_parts(buf as *const u8, size).to_vec();
            mgba_sys::free(buf);
            v
        }
    }

    /// Restore this player's lockstep state. Call after the owning core's
    /// `load_state`.
    pub fn load_state(&mut self, state: &[u8]) -> bool {
        unsafe {
            // The core load cleared the timing list; make sure our event
            // isn't considered scheduled twice if a caller restores a driver
            // blob without a core load in between.
            let sio = self.raw.d.p;
            if !sio.is_null() {
                let gba = (*sio).p;
                mgba_sys::mTimingDeschedule(&mut (*gba).timing, &mut self.raw.event);
            }
            (self.raw.d.loadState.unwrap())(&mut self.raw.d, state.as_ptr() as *const _, state.len())
        }
    }
}
