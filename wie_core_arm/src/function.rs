use alloc::{boxed::Box, string::String};
use core::{fmt::Debug, future::Future, marker::PhantomData};

use wie_base::util::read_null_terminated_string;

use crate::{core::ArmCoreResult, ArmCore};

#[async_trait::async_trait(?Send)]
pub trait RegisteredFunction {
    async fn call(&self, core: &mut ArmCore) -> ArmCoreResult<()>;
}

pub struct RegisteredFunctionHolder<F, P, E, C, R>
where
    F: EmulatedFunction<P, E, C, R> + 'static,
    E: Debug,
    C: Clone + 'static,
    R: ResultWriter<R>,
{
    function: Box<F>,
    context: C,
    _phantom: PhantomData<(P, E, C, R)>,
}

impl<F, P, E, C, R> RegisteredFunctionHolder<F, P, E, C, R>
where
    F: EmulatedFunction<P, E, C, R> + 'static,
    E: Debug,
    C: Clone + 'static,
    R: ResultWriter<R>,
{
    pub fn new(function: F, context: &C) -> Self {
        Self {
            function: Box::new(function),
            context: context.clone(),
            _phantom: PhantomData,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl<F, P, E, C, R> RegisteredFunction for RegisteredFunctionHolder<F, P, E, C, R>
where
    F: EmulatedFunction<P, E, C, R> + 'static,
    E: Debug,
    C: Clone + 'static,
    R: ResultWriter<R>,
{
    async fn call(&self, core: &mut ArmCore) -> ArmCoreResult<()> {
        let (pc, lr) = core.read_pc_lr()?;

        tracing::trace!("Registered function called at {:#x}, LR: {:#x}", pc, lr);

        let mut new_context = self.context.clone();

        let result = self.function.call(core, &mut new_context).await.map_err(|x| anyhow::anyhow!("{:?}", x))?;
        R::write(core, result, lr)?;

        Ok(())
    }
}

trait FnHelper<'a, E, C, R, P> {
    type Output: Future<Output = Result<R, E>> + 'a;
    fn do_call(&self, core: &'a mut ArmCore, context: &'a mut C) -> Self::Output;
}

impl<'a, E, C, R, F, Fut> FnHelper<'a, E, C, R, ()> for F
where
    F: Fn(&'a mut ArmCore, &'a mut C) -> Fut,
    Fut: Future<Output = Result<R, E>> + 'a,
    C: 'a,
    R: 'a,
{
    type Output = Fut;
    fn do_call(&self, core: &'a mut ArmCore, context: &'a mut C) -> Fut {
        self(core, context)
    }
}

impl<'a, E, C, R, F, Fut, P0> FnHelper<'a, E, C, R, (P0,)> for F
where
    F: Fn(&'a mut ArmCore, &'a mut C, P0) -> Fut,
    Fut: Future<Output = Result<R, E>> + 'a,
    C: 'a,
    R: 'a,
    P0: EmulatedFunctionParam<P0>,
{
    type Output = Fut;
    fn do_call(&self, core: &'a mut ArmCore, context: &'a mut C) -> Fut {
        let param1 = P0::get(core, 0);

        self(core, context, param1)
    }
}

impl<'a, E, C, R, F, Fut, P0, P1> FnHelper<'a, E, C, R, (P0, P1)> for F
where
    F: Fn(&'a mut ArmCore, &'a mut C, P0, P1) -> Fut,
    Fut: Future<Output = Result<R, E>> + 'a,
    C: 'a,
    R: 'a,
    P0: EmulatedFunctionParam<P0>,
    P1: EmulatedFunctionParam<P1>,
{
    type Output = Fut;
    fn do_call(&self, core: &'a mut ArmCore, context: &'a mut C) -> Fut {
        let param1 = P0::get(core, 0);
        let param2 = P1::get(core, 1);

        self(core, context, param1, param2)
    }
}

impl<'a, E, C, R, F, Fut, P0, P1, P2> FnHelper<'a, E, C, R, (P0, P1, P2)> for F
where
    F: Fn(&'a mut ArmCore, &'a mut C, P0, P1, P2) -> Fut,
    Fut: Future<Output = Result<R, E>> + 'a,
    C: 'a,
    R: 'a,
    P0: EmulatedFunctionParam<P0>,
    P1: EmulatedFunctionParam<P1>,
    P2: EmulatedFunctionParam<P2>,
{
    type Output = Fut;
    fn do_call(&self, core: &'a mut ArmCore, context: &'a mut C) -> Fut {
        let param1 = P0::get(core, 0);
        let param2 = P1::get(core, 1);
        let param3 = P2::get(core, 2);

        self(core, context, param1, param2, param3)
    }
}

#[async_trait::async_trait(?Send)]
pub trait EmulatedFunction<P, E, C, R> {
    async fn call(&self, core: &mut ArmCore, context: &mut C) -> Result<R, E>;
}

#[async_trait::async_trait(?Send)]
impl<Func, E, C, R> EmulatedFunction<(), E, C, R> for Func
where
    Func: for<'a> FnHelper<'a, E, C, R, ()>,
{
    async fn call(&self, core: &mut ArmCore, context: &mut C) -> Result<R, E> {
        self.do_call(core, context).await
    }
}

#[async_trait::async_trait(?Send)]
impl<Func, E, C, R, P0> EmulatedFunction<(P0,), E, C, R> for Func
where
    Func: for<'a> FnHelper<'a, E, C, R, (P0,)>,
    P0: EmulatedFunctionParam<P0>,
{
    async fn call(&self, core: &mut ArmCore, context: &mut C) -> Result<R, E> {
        self.do_call(core, context).await
    }
}

#[async_trait::async_trait(?Send)]
impl<Func, E, C, R, P0, P1> EmulatedFunction<(P0, P1), E, C, R> for Func
where
    Func: for<'a> FnHelper<'a, E, C, R, (P0, P1)>,
    P0: EmulatedFunctionParam<P0>,
    P1: EmulatedFunctionParam<P1>,
{
    async fn call(&self, core: &mut ArmCore, context: &mut C) -> Result<R, E> {
        self.do_call(core, context).await
    }
}

#[async_trait::async_trait(?Send)]
impl<Func, E, C, R, P0, P1, P2> EmulatedFunction<(P0, P1, P2), E, C, R> for Func
where
    Func: for<'a> FnHelper<'a, E, C, R, (P0, P1, P2)>,
    P0: EmulatedFunctionParam<P0>,
    P1: EmulatedFunctionParam<P1>,
    P2: EmulatedFunctionParam<P2>,
{
    async fn call(&self, core: &mut ArmCore, context: &mut C) -> Result<R, E> {
        self.do_call(core, context).await
    }
}

pub trait EmulatedFunctionParam<T> {
    fn get(core: &mut ArmCore, pos: usize) -> T;

    fn read(core: &mut ArmCore, pos: usize) -> u32 {
        core.read_param(pos).unwrap()
    }
}

impl EmulatedFunctionParam<String> for String {
    fn get(core: &mut ArmCore, pos: usize) -> String {
        let ptr = Self::read(core, pos);

        read_null_terminated_string(core, ptr).unwrap()
    }
}

impl EmulatedFunctionParam<u32> for u32 {
    fn get(core: &mut ArmCore, pos: usize) -> u32 {
        Self::read(core, pos)
    }
}

pub trait ResultWriter<R> {
    fn write(core: &mut ArmCore, value: R, lr: u32) -> anyhow::Result<()>;
}

impl ResultWriter<u32> for u32 {
    fn write(core: &mut ArmCore, value: u32, lr: u32) -> anyhow::Result<()> {
        core.write_result(value, lr)
    }
}

impl ResultWriter<()> for () {
    fn write(core: &mut ArmCore, _: (), lr: u32) -> anyhow::Result<()> {
        core.write_result(0, lr)
    }
}