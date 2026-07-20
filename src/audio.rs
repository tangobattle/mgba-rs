use std::pin::Pin;

/// An mgba audio ring, viewed in place: a `#[repr(transparent)]` wrapper
/// over the C struct. Borrow one from a core
/// ([`Core::audio_buffer`](crate::core::Core::audio_buffer)) or own one
/// via [`OwnedAudioBuffer`].
#[repr(transparent)]
pub struct AudioBuffer(pub(super) mgba_sys::mAudioBuffer);

impl AudioBuffer {
    pub fn available(&self) -> usize {
        unsafe { mgba_sys::mAudioBufferAvailable(&self.0) }
    }

    pub fn read(&mut self, samples: &mut [i16], count: usize) -> usize {
        unsafe { mgba_sys::mAudioBufferRead(&mut self.0, samples.as_mut_ptr(), count) }
    }

    pub fn write(&mut self, samples: &[i16], count: usize) -> usize {
        unsafe { mgba_sys::mAudioBufferWrite(&mut self.0, samples.as_ptr(), count) }
    }

    pub fn channels(&self) -> u32 {
        self.0.channels
    }

    pub fn clear(&mut self) {
        unsafe { mgba_sys::mAudioBufferClear(&mut self.0) }
    }

    pub fn as_mut_ptr(&mut self) -> *mut mgba_sys::mAudioBuffer {
        &mut self.0
    }
}

/// An audio ring this crate allocates and deinitializes (the buffers a
/// resampler writes into). Derefs to [`AudioBuffer`] for everything else.
pub struct OwnedAudioBuffer {
    // Pinned: the C resampler stores the buffer pointer across calls.
    inner: Pin<Box<AudioBuffer>>,
}

unsafe impl Send for OwnedAudioBuffer {}

impl OwnedAudioBuffer {
    pub fn new(capacity: usize, channels: u32) -> Self {
        let mut inner: Pin<Box<AudioBuffer>> = Box::pin(AudioBuffer(unsafe { std::mem::zeroed() }));
        unsafe {
            mgba_sys::mAudioBufferInit(inner.as_mut().get_unchecked_mut().as_mut_ptr(), capacity, channels);
        }
        OwnedAudioBuffer { inner }
    }
}

impl std::ops::Deref for OwnedAudioBuffer {
    type Target = AudioBuffer;

    fn deref(&self) -> &AudioBuffer {
        &self.inner
    }
}

impl std::ops::DerefMut for OwnedAudioBuffer {
    fn deref_mut(&mut self) -> &mut AudioBuffer {
        // The ring is heap-pinned; handing out &mut never moves it.
        unsafe { self.inner.as_mut().get_unchecked_mut() }
    }
}

impl Drop for OwnedAudioBuffer {
    fn drop(&mut self) {
        unsafe { mgba_sys::mAudioBufferDeinit(self.as_mut_ptr()) }
    }
}

pub struct AudioResampler {
    inner: Pin<Box<mgba_sys::mAudioResampler>>,
}

unsafe impl Send for AudioResampler {}

impl AudioResampler {
    pub fn new() -> Self {
        let mut inner: Pin<Box<mgba_sys::mAudioResampler>> = Box::pin(unsafe { std::mem::zeroed() });
        unsafe {
            mgba_sys::mAudioResamplerInit(
                inner.as_mut().get_unchecked_mut(),
                mgba_sys::mInterpolatorType_mINTERPOLATOR_SINC,
            );
        }
        AudioResampler { inner }
    }

    /// Sets the source buffer + its sample rate. The C layer stores
    /// the pointer for use by subsequent [`Self::process`] calls — the
    /// caller must ensure `source` stays live until either `process`
    /// runs or a new source is set.
    pub fn set_source(&mut self, source: &mut AudioBuffer, rate: f64, consume: bool) {
        unsafe {
            mgba_sys::mAudioResamplerSetSource(
                self.inner.as_mut().get_unchecked_mut(),
                source.as_mut_ptr(),
                rate,
                consume,
            );
        }
    }

    /// Sets the destination buffer + its sample rate. As with
    /// [`Self::set_source`], the C layer stores the pointer across
    /// calls — the destination must outlive any later `process`.
    pub fn set_destination(&mut self, destination: &mut AudioBuffer, rate: f64) {
        unsafe {
            mgba_sys::mAudioResamplerSetDestination(
                self.inner.as_mut().get_unchecked_mut(),
                destination.as_mut_ptr(),
                rate,
            );
        }
    }

    pub fn process(&mut self) -> usize {
        unsafe { mgba_sys::mAudioResamplerProcess(self.inner.as_mut().get_unchecked_mut()) }
    }
}

impl Default for AudioResampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioResampler {
    fn drop(&mut self) {
        unsafe { mgba_sys::mAudioResamplerDeinit(self.inner.as_mut().get_unchecked_mut()) }
    }
}
