use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::mem::size_of;

use bytemuck::{Pod, Zeroable};

use wie_backend::Backend;
use wie_base::util::{read_generic, write_generic, ByteRead};
use wie_core_arm::{Allocator, ArmCore};
use wie_impl_java::{r#impl::java::lang::String as JavaString, JavaContext};

use crate::runtime::java::context::{JavaFullName, KtfJavaContext};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WIPIJBInterface {
    unk1: u32,
    fn_java_jump_1: u32,
    fn_java_jump_2: u32,
    fn_java_jump_3: u32,
    fn_get_java_method: u32,
    fn_get_static_field: u32,
    fn_unk4: u32,
    fn_unk5: u32,
    fn_unk7: u32,
    fn_unk8: u32,
    fn_register_class: u32,
    fn_register_java_string: u32,
    fn_call_native: u32,
}

pub fn get_wipi_jb_interface(core: &mut ArmCore, backend: &Backend) -> anyhow::Result<u32> {
    let interface = WIPIJBInterface {
        unk1: 0,
        fn_java_jump_1: core.register_function(java_jump_1, backend)?,
        fn_java_jump_2: core.register_function(java_jump_2, backend)?,
        fn_java_jump_3: core.register_function(java_jump_3, backend)?,
        fn_get_java_method: core.register_function(get_java_method, backend)?,
        fn_get_static_field: core.register_function(get_static_field, backend)?,
        fn_unk4: core.register_function(jb_unk4, backend)?,
        fn_unk5: core.register_function(jb_unk5, backend)?,
        fn_unk7: core.register_function(jb_unk7, backend)?,
        fn_unk8: core.register_function(jb_unk8, backend)?,
        fn_register_class: core.register_function(register_class, backend)?,
        fn_register_java_string: core.register_function(register_java_string, backend)?,
        fn_call_native: core.register_function(call_native, backend)?,
    };

    let address = Allocator::alloc(core, size_of::<WIPIJBInterface>() as u32)?;
    write_generic(core, address, interface)?;

    Ok(address)
}

pub async fn java_class_load(core: &mut ArmCore, backend: &mut Backend, ptr_target: u32, name: String) -> anyhow::Result<u32> {
    tracing::trace!("load_java_class({:#x}, {})", ptr_target, name);

    let class = KtfJavaContext::new(core, backend).load_class(&name).await?;

    if let Some(x) = class {
        write_generic(core, ptr_target, x.ptr_raw)?;

        Ok(0)
    } else {
        tracing::error!("load_java_class({}) failed", name);

        Ok(1)
    }
}

pub async fn java_throw(_: &mut ArmCore, _: &mut Backend, error: String, a1: u32) -> anyhow::Result<u32> {
    tracing::error!("java_throw({}, {})", error, a1);

    anyhow::bail!("Java Exception thrown {}, {:#x}", error, a1)
}

async fn get_java_method(core: &mut ArmCore, backend: &mut Backend, ptr_class: u32, ptr_fullname: u32) -> anyhow::Result<u32> {
    let fullname = JavaFullName::from_ptr(core, ptr_fullname)?;
    tracing::trace!("get_java_method({:#x}, {})", ptr_class, fullname);

    let context = KtfJavaContext::new(core, backend);
    let class = context.class_from_raw(ptr_class);
    let method = class.method(&context, &fullname)?.unwrap();

    tracing::trace!("get_java_method result {:#x}", method.ptr_raw);

    Ok(method.ptr_raw)
}

async fn java_jump_1(core: &mut ArmCore, _: &mut Backend, arg1: u32, address: u32) -> anyhow::Result<u32> {
    tracing::trace!("java_jump_1({:#x}, {:#x})", arg1, address);

    core.run_function::<u32>(address, &[arg1]).await
}

async fn register_class(core: &mut ArmCore, backend: &mut Backend, ptr_class: u32) -> anyhow::Result<()> {
    tracing::trace!("register_class({:#x})", ptr_class);

    let mut context = KtfJavaContext::new(core, backend);
    let class = context.class_from_raw(ptr_class);
    context.register_class(&class).await?;

    Ok(())
}

async fn register_java_string(core: &mut ArmCore, backend: &mut Backend, offset: u32, length: u32) -> anyhow::Result<u32> {
    tracing::trace!("register_java_string({:#x}, {:#x})", offset, length);

    let mut cursor = offset;
    let length = if length == 0xffff_ffff {
        let length: u16 = read_generic(core, offset)?;
        cursor += 2;
        length
    } else {
        length as _
    };
    let bytes = core.read_bytes(cursor, (length * 2) as _)?;
    let bytes_u16 = bytes.chunks(2).map(|x| u16::from_le_bytes([x[0], x[1]])).collect::<Vec<_>>();

    let mut context = KtfJavaContext::new(core, backend);
    let instance = JavaString::from_utf16(&mut context, &bytes_u16).await?;

    Ok(instance.ptr_instance as u32)
}

async fn get_static_field(core: &mut ArmCore, backend: &mut Backend, ptr_class: u32, field_name: u32) -> anyhow::Result<u32> {
    tracing::warn!("stub get_static_field({:#x}, {:#x})", ptr_class, field_name);

    let field_name = JavaFullName::from_ptr(core, field_name)?;

    let context = KtfJavaContext::new(core, backend);
    let class = context.class_from_raw(ptr_class);
    let field = class.field(&context, &field_name.name)?.unwrap();

    Ok(field.ptr_raw)
}

async fn jb_unk4(_: &mut ArmCore, _: &mut Backend, a0: u32, a1: u32) -> anyhow::Result<u32> {
    tracing::warn!("stub jb_unk4({:#x}, {:#x})", a0, a1);

    Ok(0)
}

async fn jb_unk5(_: &mut ArmCore, _: &mut Backend, a0: u32, a1: u32) -> anyhow::Result<u32> {
    tracing::warn!("stub jb_unk5({:#x}, {:#x})", a0, a1);

    Ok(0)
}

async fn jb_unk7(_: &mut ArmCore, _: &mut Backend, a0: u32) -> anyhow::Result<u32> {
    tracing::warn!("stub jb_unk7({:#x})", a0);

    Ok(0)
}

async fn jb_unk8(_: &mut ArmCore, _: &mut Backend, a0: u32) -> anyhow::Result<u32> {
    tracing::warn!("stub jb_unk8({:#x})", a0);

    Ok(0)
}

async fn call_native(core: &mut ArmCore, _: &mut Backend, address: u32, ptr_data: u32) -> anyhow::Result<u32> {
    tracing::trace!("java_jump_native({:#x}, {:#x})", address, ptr_data);

    let result = core.run_function::<u32>(address, &[ptr_data]).await?;

    write_generic(core, ptr_data, result)?;
    write_generic(core, ptr_data + 4, 0u32)?;

    Ok(ptr_data)
}

async fn java_jump_2(core: &mut ArmCore, _: &mut Backend, arg1: u32, arg2: u32, address: u32) -> anyhow::Result<u32> {
    tracing::trace!("java_jump_2({:#x}, {:#x}, {:#x})", arg1, arg2, address);

    core.run_function::<u32>(address, &[arg1, arg2]).await
}

async fn java_jump_3(core: &mut ArmCore, _: &mut Backend, arg1: u32, arg2: u32, arg3: u32, address: u32) -> anyhow::Result<u32> {
    tracing::trace!("java_jump_3({:#x}, {:#x}, {:#x}, {:#x})", arg1, arg2, arg3, address);

    core.run_function::<u32>(address, &[arg1, arg2, arg3]).await
}

pub async fn java_new(core: &mut ArmCore, backend: &mut Backend, ptr_class: u32) -> anyhow::Result<u32> {
    tracing::trace!("java_new({:#x})", ptr_class);

    let mut context = KtfJavaContext::new(core, backend);
    let class = context.class_from_raw(ptr_class);
    let class_name = class.name(&context)?;

    let instance = context.instantiate(&format!("L{};", class_name)).await?;

    Ok(instance.ptr_instance as u32)
}

pub async fn java_array_new(core: &mut ArmCore, backend: &mut Backend, element_type: u32, count: u32) -> anyhow::Result<u32> {
    tracing::trace!("java_array_new({:#x}, {:#x})", element_type, count);

    let mut java_context = KtfJavaContext::new(core, backend);

    let element_type_name = if element_type > 0x100 {
        // HACK: we don't have element type class
        let class = java_context.class_from_raw(element_type);
        class.name(&java_context)?
    } else {
        (element_type as u8 as char).to_string()
    };

    let instance = java_context.instantiate_array(&element_type_name, count as _).await?;

    Ok(instance.ptr_instance as u32)
}

pub async fn java_check_cast(_: &mut ArmCore, _: &mut Backend, ptr_class: u32, ptr_instance: u32) -> anyhow::Result<u32> {
    tracing::warn!("stub java_check_cast({:#x}, {:#x})", ptr_class, ptr_instance);

    Ok(1)
}
