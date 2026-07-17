/// The GBA's ARM7TDMI, viewed in place: a `#[repr(transparent)]` wrapper
/// over the C struct, only ever handed out as `&ArmCore` / `&mut ArmCore`
/// borrowed from a [`Gba`](crate::gba::Gba).
#[repr(transparent)]
pub struct ArmCore(pub(super) mgba_sys::ARMCore);

pub enum ExecutionMode {
    ARM,
    Thumb,
}

impl ArmCore {
    pub fn gpr(&self, r: usize) -> i32 {
        unsafe { self.0.__bindgen_anon_1.__bindgen_anon_1.gprs[r] }
    }

    pub fn cpsr(&self) -> i32 {
        unsafe { self.0.__bindgen_anon_1.__bindgen_anon_1.cpsr.packed }
    }

    pub fn thumb_pc(&self) -> u32 {
        self.gpr(15) as u32 - mgba_sys::WordSize_WORD_SIZE_THUMB as u32
    }

    pub fn arm_pc(&self) -> u32 {
        self.gpr(15) as u32 - mgba_sys::WordSize_WORD_SIZE_ARM as u32
    }

    pub fn execution_mode(&self) -> ExecutionMode {
        match self.0.executionMode {
            mgba_sys::ExecutionMode_MODE_ARM => ExecutionMode::ARM,
            mgba_sys::ExecutionMode_MODE_THUMB => ExecutionMode::Thumb,
            _ => unreachable!(),
        }
    }

    pub fn set_gpr(&mut self, r: usize, v: i32) {
        unsafe {
            self.0.__bindgen_anon_1.__bindgen_anon_1.gprs[r] = v;
        }
    }

    pub fn set_thumb_pc(&mut self, v: u32) {
        self.set_gpr(15, v as i32);
        self.thumb_write_pc();
    }

    fn thumb_write_pc(&mut self) {
        let cpu: *mut mgba_sys::ARMCore = &mut self.0;
        unsafe {
            // uint32_t pc = cpu->gprs[ARM_PC] & -WORD_SIZE_THUMB;
            let mut pc = (self.gpr(mgba_sys::ARM_PC as usize)
                & -(mgba_sys::WordSize_WORD_SIZE_THUMB as i32)) as u32;

            // cpu->memory.setActiveRegion(cpu, pc);
            (*cpu).memory.setActiveRegion.unwrap()(cpu, pc);

            // LOAD_16(cpu->prefetch[0], pc & cpu->memory.activeMask, cpu->memory.activeRegion);
            (*cpu).prefetch[0] = *(((*cpu).memory.activeRegion as *const u8)
                .add((pc & (*cpu).memory.activeMask) as usize)
                as *const u16) as u32;

            // pc += WORD_SIZE_THUMB;
            pc += mgba_sys::WordSize_WORD_SIZE_THUMB as u32;

            // LOAD_16(cpu->prefetch[1], pc & cpu->memory.activeMask, cpu->memory.activeRegion);
            (*cpu).prefetch[1] = *(((*cpu).memory.activeRegion as *const u8)
                .add((pc & (*cpu).memory.activeMask) as usize)
                as *const u16) as u32;

            // cpu->gprs[ARM_PC] = pc;
            self.set_gpr(mgba_sys::ARM_PC as usize, pc as i32);
        }
    }
}
