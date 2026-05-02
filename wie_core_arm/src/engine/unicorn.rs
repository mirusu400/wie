//! ARM JIT engine backed by Unicorn (QEMU TCG).
//!
//! Provides the same `ArmEngine` surface as the in-house step interpreter,
//! but runs translated code so the host can keep up with real-world WIPI/J2ME
//! titles. Pulled in everywhere mmap RWX is allowed — Win64, Linux (incl.
//! Anbernic / Steam Deck), macOS, Android, Switch / Vita / 3DS homebrew.
//! Excluded on iOS / tvOS / wasm32 via Cargo target cfg.
//!
//! ## SVC handling
//!
//! WIE drives kernel-style traps through SVC: the platform code maps an SVC
//! handler at vector 0x08 and the engine returns to the JVM each time the
//! emulated CPU enters Supervisor mode. With Unicorn we intercept the SVC
//! instruction directly via `add_intr_hook` (intno = 2), record the call
//! site, and stop the JIT before it would jump to the vector. The caller
//! reads `lr` / `spsr` to decode the SVC byte just like in the interpreter
//! path, so the surrounding code paths don't change.
extern crate std;

use alloc::{boxed::Box, format, sync::Arc, vec::Vec};
use core::cell::RefCell;
use std::sync::Mutex;

use unicorn_engine::{Arch, HookType, Mode, Prot, RegisterARM, Unicorn};

use wie_util::{Result, WieError};

use crate::engine::{ArmEngine, ArmRegister, EngineRunResult, MemoryPermission};

/// Unicorn requires page-aligned mappings; 4 KiB is the smallest universally
/// supported size on the targets we care about.
const PAGE_SIZE: u64 = 0x1000;
const PAGE_MASK: u64 = PAGE_SIZE - 1;

/// SVC entry point used by WIE's vector layout. Mirrors `Arm32CpuEngine`.
const SVC_VECTOR: u32 = 0x08;
/// Supervisor mode bits in CPSR.
const CPSR_MODE_SVC: u32 = 0x13;
/// Thumb-state bit in CPSR (T flag, bit 5).
const CPSR_T_BIT: u32 = 1 << 5;

/// Captured by the SVC hook and consumed by the next `run()` return.
#[derive(Default)]
struct SvcRecord {
    pending: bool,
    /// Equivalent of the supervisor-mode LR after the SVC trap: address of
    /// the instruction immediately after the SVC.
    lr: u32,
    /// Snapshot of CPSR at the SVC site (User mode), used as SPSR_svc by
    /// the caller.
    spsr: u32,
}

pub struct UnicornArmEngine {
    uc: Unicorn<'static, ()>,
    svc: Arc<Mutex<SvcRecord>>,
    /// Set of mapped page base addresses, used to answer `is_mapped` cheaply
    /// without round-tripping through unicorn.
    mapped: RefCell<alloc::collections::BTreeSet<u32>>,
}

impl UnicornArmEngine {
    pub fn new() -> Self {
        let mut uc = Unicorn::new(Arch::ARM, Mode::ARM).expect("Unicorn::new failed");
        let svc: Arc<Mutex<SvcRecord>> = Arc::new(Mutex::new(SvcRecord::default()));
        let svc_clone = Arc::clone(&svc);

        // intno == 2 → ARM SVC (a.k.a. SWI). Stop the JIT here so the caller
        // can run the SVC handler outside the emulated CPU.
        // Lazy page-mapper: WIE allocates pages out-of-band on the JVM side
        // and the step interpreter never refuses an access — it just falls
        // through to the underlying page table with whatever data is there.
        // Unicorn is stricter and faults on any unmapped access, so we
        // create the page on demand and let the access retry. This keeps
        // the JIT in lock-step with how the interpreter behaves on the
        // same WIE allocator path.
        uc.add_mem_hook(HookType::MEM_UNMAPPED, 1, 0, |uc, _kind, address, _size, _value| {
            let page_start = address & !PAGE_MASK;
            let _ = uc.mem_map(page_start, PAGE_SIZE, Prot::ALL);
            true
        })
        .expect("mem-unmapped hook install failed");

        uc.add_intr_hook(move |uc, intno| {
            if intno != 2 {
                return;
            }
            let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
            let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
            let mut rec = svc_clone.lock().unwrap();
            rec.pending = true;
            rec.lr = pc; // PC has already advanced past the SVC instruction.
            rec.spsr = cpsr;
            // Drop the lock before stopping so we don't hold it across the
            // unicorn callback boundary.
            drop(rec);
            let _ = uc.emu_stop();
        })
        .expect("intr hook install failed");

        Self {
            uc,
            svc,
            mapped: RefCell::new(alloc::collections::BTreeSet::new()),
        }
    }

    fn permission_to_unicorn(perm: MemoryPermission) -> Prot {
        match perm {
            MemoryPermission::ReadExecute => Prot::READ | Prot::EXEC,
            MemoryPermission::ReadWrite => Prot::READ | Prot::WRITE,
            MemoryPermission::ReadWriteExecute => Prot::ALL,
        }
    }

    fn arm_register_to_unicorn(reg: ArmRegister) -> RegisterARM {
        match reg {
            ArmRegister::R0 => RegisterARM::R0,
            ArmRegister::R1 => RegisterARM::R1,
            ArmRegister::R2 => RegisterARM::R2,
            ArmRegister::R3 => RegisterARM::R3,
            ArmRegister::R4 => RegisterARM::R4,
            ArmRegister::R5 => RegisterARM::R5,
            ArmRegister::R6 => RegisterARM::R6,
            ArmRegister::R7 => RegisterARM::R7,
            ArmRegister::R8 => RegisterARM::R8,
            ArmRegister::SB => RegisterARM::R9,
            ArmRegister::SL => RegisterARM::R10,
            ArmRegister::FP => RegisterARM::R11,
            ArmRegister::IP => RegisterARM::R12,
            ArmRegister::SP => RegisterARM::SP,
            ArmRegister::LR => RegisterARM::LR,
            ArmRegister::PC => RegisterARM::PC,
            ArmRegister::Cpsr => RegisterARM::CPSR,
        }
    }

    /// Detect the “PC == 0x08, mode == Supervisor” state used by the
    /// interpreter to mean the CPU just trapped via SVC. Unicorn exits its
    /// SVC hook *before* taking the vector, so this is mostly a fallback for
    /// the rare case where the JIT would land here on its own.
    fn is_at_svc_vector(&self) -> bool {
        let pc = self.uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
        let cpsr = self.uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
        pc == SVC_VECTOR && (cpsr & 0x1f) == CPSR_MODE_SVC
    }
}

impl Default for UnicornArmEngine {
    fn default() -> Self {
        Self::new()
    }
}

// Unicorn's instance handle is `Rc<UnsafeCell<...>>` (a single-threaded
// object), so the crate marks it `!Send`. The wider WIE pipeline runs the
// emulator on a single dedicated worker thread; ownership transfers to that
// worker once at construction and never crosses thread boundaries again.
// The `Send` impl is sound for that one-thread-at-a-time use; concurrent
// use from multiple threads would still be UB.
unsafe impl Send for UnicornArmEngine {}

impl ArmEngine for UnicornArmEngine {
    fn run(&mut self, end: u32, count: u32) -> Result<EngineRunResult> {
        self.svc.lock().unwrap().pending = false;

        let pc = self.uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
        if pc < 0x1000 {
            return Err(WieError::InvalidMemoryAccess(pc));
        }
        if pc == end {
            return Ok(EngineRunResult::End);
        }
        if self.is_at_svc_vector() {
            let lr = self.uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
            let spsr = self.uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
            return decode_svc_at(self, lr, spsr);
        }

        let cpsr = self.uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
        let begin = if (cpsr & CPSR_T_BIT) != 0 { (pc | 1) as u64 } else { pc as u64 };

        match self.uc.emu_start(begin, end as u64, 0, count as usize) {
            Ok(()) => {
                if let Some((lr, spsr)) = take_svc(&self.svc) {
                    return decode_svc_at(self, lr, spsr);
                }
                let new_pc = self.uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                if new_pc == end {
                    Ok(EngineRunResult::End)
                } else {
                    Ok(EngineRunResult::CountExhausted)
                }
            }
            Err(e) => Err(WieError::FatalError(format!("unicorn emu_start: {e:?}"))),
        }
    }

    fn reg_write(&mut self, reg: ArmRegister, value: u32) {
        // Match the interpreter's PC-with-Thumb convention: setting PC to an
        // odd address means “jump to Thumb code”.
        if reg == ArmRegister::PC && (value & 1) == 1 {
            let _ = self.uc.reg_write(RegisterARM::PC, (value & !1) as u64);
            let cpsr = self.uc.reg_read(RegisterARM::CPSR).unwrap_or(0);
            let _ = self.uc.reg_write(RegisterARM::CPSR, cpsr | CPSR_T_BIT as u64);
            return;
        }
        let ureg = Self::arm_register_to_unicorn(reg);
        let _ = self.uc.reg_write(ureg, value as u64);
    }

    fn reg_read(&self, reg: ArmRegister) -> u32 {
        let ureg = Self::arm_register_to_unicorn(reg);
        self.uc.reg_read(ureg).unwrap_or(0) as u32
    }

    fn mem_map(&mut self, address: u32, size: usize, permission: MemoryPermission) {
        let aligned_start = (address as u64) & !PAGE_MASK;
        let aligned_end = ((address as u64) + size as u64 + PAGE_MASK) & !PAGE_MASK;
        let aligned_size = aligned_end - aligned_start;

        // Skip pages that are already mapped — unicorn errors on overlap.
        let mut to_map = Vec::new();
        let mut mapped = self.mapped.borrow_mut();
        let mut cursor = aligned_start;
        while cursor < aligned_end {
            if !mapped.contains(&(cursor as u32)) {
                to_map.push(cursor);
            }
            cursor += PAGE_SIZE;
        }
        if to_map.is_empty() {
            return;
        }

        // Coalesce contiguous runs to minimize unicorn map calls.
        let mut run_start = to_map[0];
        let mut run_end = run_start + PAGE_SIZE;
        let perm = Self::permission_to_unicorn(permission);
        for &page in &to_map[1..] {
            if page == run_end {
                run_end += PAGE_SIZE;
            } else {
                let _ = self.uc.mem_map(run_start, run_end - run_start, perm);
                run_start = page;
                run_end = page + PAGE_SIZE;
            }
        }
        let _ = self.uc.mem_map(run_start, run_end - run_start, perm);

        for page in to_map {
            mapped.insert(page as u32);
        }
        let _ = aligned_size;
    }

    fn mem_write(&mut self, address: u32, data: &[u8]) -> Result<()> {
        self.uc
            .mem_write(address as u64, data)
            .map_err(|_| WieError::InvalidMemoryAccess(address))
    }

    fn mem_read(&mut self, address: u32, size: usize, result: &mut [u8]) -> Result<usize> {
        self.uc
            .mem_read(address as u64, &mut result[..size])
            .map_err(|_| WieError::InvalidMemoryAccess(address))?;
        Ok(size)
    }

    fn is_mapped(&self, address: u32, size: usize) -> bool {
        let mapped = self.mapped.borrow();
        let mut cursor = (address as u64) & !PAGE_MASK;
        let limit = (address as u64) + size as u64;
        while cursor < limit {
            if !mapped.contains(&(cursor as u32)) {
                return false;
            }
            cursor += PAGE_SIZE;
        }
        true
    }
}

fn take_svc(svc: &Arc<Mutex<SvcRecord>>) -> Option<(u32, u32)> {
    let mut rec = svc.lock().unwrap();
    if !rec.pending {
        return None;
    }
    rec.pending = false;
    Some((rec.lr, rec.spsr))
}

/// Decode the SVC instruction at LR-2 (Thumb only — the WIE platform code
/// emits Thumb SVCs). Mirrors `Arm32CpuEngine::read_svc_result`.
fn decode_svc_at(engine: &UnicornArmEngine, lr: u32, spsr: u32) -> Result<EngineRunResult> {
    let svc_address = lr.checked_sub(2).ok_or(WieError::InvalidMemoryAccess(lr))?;
    let mut bytes = [0u8; 2];
    engine
        .uc
        .mem_read(svc_address as u64, &mut bytes)
        .map_err(|_| WieError::InvalidMemoryAccess(svc_address))?;
    let instruction = u16::from_le_bytes(bytes);
    if instruction & 0xff00 != 0xdf00 {
        return Err(WieError::FatalError(format!("Invalid Thumb SVC instruction {instruction:#06x}")));
    }
    let category = instruction as u32 & 0xff;
    Ok(EngineRunResult::Svc { category, lr, spsr })
}
