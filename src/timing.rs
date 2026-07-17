/// A core's `mTiming`, viewed in place: a `#[repr(transparent)]` wrapper
/// only ever handed out as `&Timing` borrowed from a
/// [`Gba`](crate::gba::Gba).
#[repr(transparent)]
pub struct Timing(pub(super) mgba_sys::mTiming);

impl Timing {
    pub fn current_time(&self) -> i32 {
        unsafe { mgba_sys::mTimingCurrentTime(&self.0) }
    }
}
