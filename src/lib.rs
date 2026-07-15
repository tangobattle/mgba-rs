pub mod arm_core;
pub mod audio;
pub mod core;
pub mod gba;
pub mod input;
pub mod log;
pub mod sio;
pub mod state;
pub mod sync;
pub mod thread;
pub mod timing;
// Private on purpose: a trapper splices itself into its core's CPU
// component table with no uninstall, and core deinit walks that table —
// a trapper owned anywhere but inside the core can be freed first and
// turn core teardown into a call through reclaimed memory. The only way
// to install traps is `core::Core::set_traps`, which gives the trapper
// exactly the core's lifetime.
mod trapper;
pub mod vfile;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("call to {0} failed")]
    CallFailed(&'static str),
}
