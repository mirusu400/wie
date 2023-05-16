use core::mem::size_of;

use wie_base::util::{read_generic, round_up, write_generic};

use crate::core::{ArmCore, HEAP_BASE};

const HEAP_SIZE: u32 = 0x100000;

#[derive(Clone, Copy)]
struct AllocationHeader {
    size: u32,
    in_use: u32,
}

pub struct Allocator {}

impl Allocator {
    pub fn init(core: &mut ArmCore) -> anyhow::Result<(u32, u32)> {
        core.alloc(HEAP_BASE, HEAP_SIZE)?;

        let header = AllocationHeader { size: HEAP_SIZE, in_use: 0 };

        write_generic(core, HEAP_BASE, header)?;

        Ok((HEAP_BASE, HEAP_SIZE))
    }

    pub fn alloc(core: &mut ArmCore, size: u32) -> anyhow::Result<u32> {
        let alloc_size = round_up(size as usize + size_of::<AllocationHeader>(), 4) as u32;

        let address = Self::find_address(core, alloc_size).ok_or_else(|| anyhow::anyhow!("Failed to allocate"))?;

        let previous_header = read_generic::<AllocationHeader>(core, address)?;

        let header = AllocationHeader { size: alloc_size, in_use: 1 };
        write_generic(core, address, header)?;

        // write next
        if previous_header.size > alloc_size {
            let next_header = AllocationHeader {
                size: previous_header.size - alloc_size,
                in_use: 0,
            };
            write_generic(core, address + alloc_size, next_header)?;
        }

        Ok(address + 8)
    }

    pub fn free(core: &mut ArmCore, address: u32) -> anyhow::Result<()> {
        let base_address = address - 8;

        let header = read_generic::<AllocationHeader>(core, base_address)?;
        assert!(header.in_use == 1);

        let header = AllocationHeader {
            size: header.size,
            in_use: 0,
        };
        write_generic(core, base_address, header)?;

        Ok(())
    }

    fn find_address(core: &mut ArmCore, request_size: u32) -> Option<u32> {
        let mut cursor = HEAP_BASE;
        loop {
            let header = read_generic::<AllocationHeader>(core, cursor).ok()?;
            if header.in_use == 0 && header.size >= request_size {
                return Some(cursor);
            } else {
                cursor += header.size;
            }

            if cursor >= HEAP_BASE + HEAP_SIZE {
                break;
            }
        }

        None
    }
}