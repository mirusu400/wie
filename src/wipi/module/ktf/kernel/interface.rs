use std::mem::size_of;

use crate::{core::arm::ArmCore, wipi::module::ktf::kernel::types::WIPICInterface};

use super::{
    java::{get_java_method, jb_unk1, jb_unk3},
    types::{WIPICKnlInterface, WIPIJBInterface},
    Context,
};

pub fn get_interface(core: &mut ArmCore, context: &Context, r#struct: String) -> u32 {
    log::debug!("get_interface({})", r#struct);

    match r#struct.as_str() {
        "WIPIC_knlInterface" => get_wipic_knl_interface(core, context),
        "WIPI_JBInterface" => get_wipi_jb_interface(core, context),
        _ => {
            log::warn!("Unknown {}", r#struct);
            log::warn!("Register dump\n{}", core.dump_regs().unwrap());

            0
        }
    }
}

fn get_wipic_knl_interface(core: &mut ArmCore, context: &Context) -> u32 {
    let knl_interface = WIPICKnlInterface {
        unk: [0; 33],
        fn_get_wipic_interfaces: core.register_function(get_wipic_interfaces, context).unwrap(),
    };

    let address = context.borrow_mut().allocator.alloc(size_of::<WIPICKnlInterface>() as u32).unwrap();
    core.write(address, knl_interface).unwrap();

    address
}

fn get_wipi_jb_interface(core: &mut ArmCore, context: &Context) -> u32 {
    let interface = WIPIJBInterface {
        unk1: 0,
        fn_unk1: core.register_function(jb_unk1, context).unwrap(),
        unk2: 0,
        unk3: 0,
        fn_unk2: core.register_function(get_java_method, context).unwrap(),
        unk: [0; 6],
        fn_unk3: core.register_function(jb_unk3, context).unwrap(),
    };

    let address = context.borrow_mut().allocator.alloc(size_of::<WIPIJBInterface>() as u32).unwrap();
    core.write(address, interface).unwrap();

    address
}

fn get_wipic_interfaces(core: &mut ArmCore, context: &Context) -> u32 {
    log::debug!("get_wipic_interfaces");

    let interface = WIPICInterface {
        interface_0: 0,
        interface_1: 0,
        interface_2: 0,
        interface_3: 0,
        interface_4: 0,
        interface_5: 0,
        interface_6: 0,
        interface_7: 0,
        interface_8: 0,
        interface_9: 0,
        interface_10: 0,
        interface_11: 0,
        interface_12: 0,
    };

    let address = context.borrow_mut().allocator.alloc(size_of::<WIPICInterface>() as u32).unwrap();

    core.write(address, interface).unwrap();

    address
}