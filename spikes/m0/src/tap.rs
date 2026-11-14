//! Rust side of the macOS process-tap shim (tap.m).
use std::ffi::c_void;
use tubeamp_eq::{EqParams, EqProcessor};

pub static PARAMS: EqParams = EqParams::new();

type Cb = extern "C" fn(*const f32, u32, *mut f32, u32, u32, *mut c_void);
extern "C" {
    fn ta_start(owner: i32, cb: Cb, user: *mut c_void) -> i32;
    fn ta_count_owned(owner: i32) -> i32;
}

extern "C" fn on_audio(inp: *const f32, in_ch: u32, out: *mut f32, out_ch: u32, frames: u32, user: *mut c_void) {
    let eq = unsafe { &mut *(user as *mut EqProcessor) };
    let (ic, oc, n) = (in_ch as usize, out_ch as usize, frames as usize);
    let input = unsafe { std::slice::from_raw_parts(inp, n * ic) };
    let output = unsafe { std::slice::from_raw_parts_mut(out, n * oc) };
    eq.process(&PARAMS, input, ic, output, oc, n);
}

/// Returns the device sample rate (>0) or a negative error code.
pub fn start() -> i32 {
    let me = std::process::id() as i32;
    unsafe { ta_count_owned(me) };
    // Sample rate is only known after start; 48k is the common default and the
    // processor is rebuilt below once we know the real rate.
    let eq = Box::into_raw(Box::new(EqProcessor::new(48000.0)));
    let sr = unsafe { ta_start(me, on_audio, eq as *mut c_void) };
    if sr > 0 && sr != 48000 {
        unsafe { *eq = EqProcessor::new(sr as f32) };
    }
    sr
}
