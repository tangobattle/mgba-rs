use super::core;

#[repr(transparent)]
pub struct Trapper(Box<TrapperCStruct>);

#[repr(C)]
struct TrapperCStruct {
    cpu_component: mgba_sys::mCPUComponent,
    real_bkpt16: Option<unsafe extern "C" fn(*mut mgba_sys::ARMCore, i32)>,
    r#impl: Impl,
}

struct Trap {
    handler: Box<dyn Fn(&mut core::Core)>,
    original: u16,
}

struct Impl {
    traps: std::collections::HashMap<u32, Trap>,
    core_ptr: *mut mgba_sys::mCore,
}

unsafe impl Send for TrapperCStruct {}
unsafe impl Send for Impl {}

const TRAPPER_IMM: i32 = 0xef;

unsafe extern "C" fn c_trapper_init(_cpu: *mut std::os::raw::c_void, _cpu_component: *mut mgba_sys::mCPUComponent) {}

unsafe extern "C" fn c_trapper_deinit(_cpu_component: *mut mgba_sys::mCPUComponent) {}

unsafe extern "C" fn c_trapper_bkpt16(arm_core: *mut mgba_sys::ARMCore, imm: i32) {
    let components = std::slice::from_raw_parts_mut(
        (*arm_core).components,
        mgba_sys::mCPUComponentType_CPU_COMPONENT_MAX as usize,
    );
    let trapper =
        &mut *(components[mgba_sys::mCPUComponentType_CPU_COMPONENT_MISC_1 as usize] as *mut _ as *mut TrapperCStruct);

    if imm != TRAPPER_IMM {
        trapper.real_bkpt16.unwrap()(arm_core, imm);
        return;
    }

    let r#impl = &mut trapper.r#impl;
    let caller = (*arm_core).__bindgen_anon_1.__bindgen_anon_1.gprs[15] as u32
        - mgba_sys::WordSize_WORD_SIZE_THUMB as u32 * 2;
    let trap = r#impl.traps.get_mut(&caller).unwrap();
    mgba_sys::ARMRunFake(arm_core, trap.original as u32);
    (trap.handler)(core::Core::from_raw_mut(r#impl.core_ptr));
}

impl Trapper {
    pub fn new(core_ptr: *mut mgba_sys::mCore, handlers: Vec<(u32, Box<dyn Fn(&mut core::Core)>)>) -> Self {
        let mut cpu_component = unsafe { std::mem::zeroed::<mgba_sys::mCPUComponent>() };
        cpu_component.init = Some(c_trapper_init);
        cpu_component.deinit = Some(c_trapper_deinit);
        let mut trapper_c_struct = Box::new(TrapperCStruct {
            cpu_component,
            real_bkpt16: None,
            r#impl: Impl {
                traps: std::collections::HashMap::new(),
                core_ptr,
            },
        });

        let arm_core = unsafe { (*((*core_ptr).board as *mut mgba_sys::GBA)).cpu };
        unsafe {
            let arm_core = &mut *arm_core;
            trapper_c_struct.real_bkpt16 = arm_core.irqh.bkpt16;
            let components = std::slice::from_raw_parts_mut(
                arm_core.components,
                mgba_sys::mCPUComponentType_CPU_COMPONENT_MAX as usize,
            );
            components[mgba_sys::mCPUComponentType_CPU_COMPONENT_MISC_1 as usize] =
                &mut *trapper_c_struct as *mut _ as *mut mgba_sys::mCPUComponent;
            mgba_sys::ARMHotplugAttach(arm_core, mgba_sys::mCPUComponentType_CPU_COMPONENT_MISC_1 as _);
            arm_core.irqh.bkpt16 = Some(c_trapper_bkpt16);
        }

        for (addr, handler) in handlers {
            match trapper_c_struct.r#impl.traps.entry(addr) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    panic!("attempting to install a second trap at 0x{:08x}", addr);
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    let mut original = 0i16;
                    unsafe {
                        mgba_sys::GBAPatch16(arm_core, addr, (0xbe00 | TRAPPER_IMM) as i16, &mut original)
                    };
                    e.insert(Trap {
                        original: original as u16,
                        handler,
                    });
                }
            };
        }
        Trapper(trapper_c_struct)
    }
}
