#![allow(unused_features)]

use std::marker::PhantomData;

pub struct Suspend<ReturnType, Effects = ()>(PhantomData<(ReturnType, Effects)>);

pub trait Effect: Sized {
    type Resume: Sized;
}

pub use effects_macros::suspend;

#[macro_export]
macro_rules! forward {
    ($sub_coroutine:expr) => {{
        let mut arg = ::core::default::Default::default();
        let mut child = $sub_coroutine;
        loop {
            match ::core::pin::Pin::new(&mut child).resume(arg) {
                ::std::ops::CoroutineState::Yielded(val) => {
                    yield val;
                }
                ::std::ops::CoroutineState::Complete(ret) => {
                    break ret;
                }
            }
        }
    }};
}
