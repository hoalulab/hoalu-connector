use crate::engine::Engine;
use crate::model::{Bar, RecentQuote};
use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

// ---- repr(C) structures; these must match market_core.h exactly ----

#[repr(C)]
pub struct MDBar {
    pub unix_microseconds: i64,
    pub open: f32,
    pub high: f32,
    pub low: f32,
    pub close: f32,
    pub volume: f32,
    pub open_interest: f32,
    pub aux1: f32,
    pub aux2: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MDRecent {
    pub last: f32,
    pub open: f32,
    pub high: f32,
    pub low: f32,
    pub previous_close: f32,
    pub change: f32,
    pub change_percent: f32,
    pub bid: f32,
    pub ask: f32,
    pub trade_volume: f32,
    pub total_volume: f32,
    pub bid_size: i32,
    pub ask_size: i32,
    pub unix_microseconds: i64,
    pub flags: i32,
}

impl From<&Bar> for MDBar {
    fn from(b: &Bar) -> Self {
        MDBar {
            unix_microseconds: b.unix_microseconds,
            open: b.open as f32,
            high: b.high as f32,
            low: b.low as f32,
            close: b.close as f32,
            volume: b.volume as f32,
            open_interest: b.open_interest as f32,
            aux1: b.aux1 as f32,
            aux2: b.aux2 as f32,
        }
    }
}

impl From<&RecentQuote> for MDRecent {
    fn from(r: &RecentQuote) -> Self {
        MDRecent {
            last: r.last as f32,
            open: r.open as f32,
            high: r.high as f32,
            low: r.low as f32,
            previous_close: r.previous_close as f32,
            change: r.change as f32,
            change_percent: r.change_percent as f32,
            bid: r.bid as f32,
            ask: r.ask as f32,
            trade_volume: r.trade_volume as f32,
            total_volume: r.total_volume as f32,
            bid_size: r.bid_size,
            ask_size: r.ask_size,
            unix_microseconds: r.unix_microseconds,
            flags: r.flags,
        }
    }
}

pub type MDUpdateCallback =
    extern "C" fn(context: *mut c_void, symbol: *const c_char, recent: *const MDRecent);

/// Wrap the raw callback pointer so it can be Send/Sync across Rust threads.
/// This is safe because C++ keeps `context` alive for the handle's lifetime and
/// the callback implements its own C++-side thread safety, as documented in README.
pub struct CallbackHandle {
    callback: MDUpdateCallback,
    context: *mut c_void,
}

unsafe impl Send for CallbackHandle {}
unsafe impl Sync for CallbackHandle {}

impl CallbackHandle {
    pub fn invoke(&self, symbol: &str, recent: &RecentQuote) {
        let Ok(c_symbol) = CString::new(symbol) else {
            return;
        };
        let md_recent: MDRecent = recent.into();
        (self.callback)(
            self.context,
            c_symbol.as_ptr(),
            &md_recent as *const MDRecent,
        );
    }
}

fn ffi_guard<F>(engine_ptr: *mut Engine, operation: F) -> i32
where
    F: FnOnce(&Engine) -> Result<(), String> + std::panic::UnwindSafe,
{
    let engine = match unsafe { (engine_ptr as *const Engine).as_ref() } {
        Some(e) => e,
        None => return -3, // Null handle.
    };

    match catch_unwind(AssertUnwindSafe(|| operation(engine))) {
        Ok(Ok(())) => 0,
        Ok(Err(msg)) => {
            engine.set_error(&msg);
            -1
        }
        Err(_) => {
            engine.set_error("panic bên trong Rust core (đã bị chặn tại FFI boundary)");
            -2
        }
    }
}

unsafe fn cstr_to_string(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("con trỏ symbol null".to_string());
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map(|s| s.to_string())
        .map_err(|_| "symbol không phải UTF-8 hợp lệ".to_string())
}

// ---- Exported C ABI ----

#[no_mangle]
pub extern "C" fn md_create(
    callback: MDUpdateCallback,
    callback_context: *mut c_void,
    output: *mut *mut c_void,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let handle = CallbackHandle {
            callback,
            context: callback_context,
        };
        let engine = Box::new(Engine::new(handle));
        Box::into_raw(engine) as *mut c_void
    }));

    match result {
        Ok(ptr) => {
            if output.is_null() {
                return -3;
            }
            unsafe { *output = ptr };
            0
        }
        Err(_) => -2,
    }
}

#[no_mangle]
pub extern "C" fn md_start(handle: *mut c_void) -> i32 {
    ffi_guard(handle as *mut Engine, |engine| engine.start())
}

#[no_mangle]
pub extern "C" fn md_stop(handle: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(engine) = unsafe { (handle as *const Engine).as_ref() } {
            engine.stop();
        }
    }));
}

#[no_mangle]
pub extern "C" fn md_destroy(handle: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() {
            return;
        }
        // Ensure the runtime is stopped before dropping the engine.
        if let Some(engine) = unsafe { (handle as *const Engine).as_ref() } {
            engine.stop();
        }
        unsafe {
            drop(Box::from_raw(handle as *mut Engine));
        }
    }));
}

#[no_mangle]
pub extern "C" fn md_request_symbol(handle: *mut c_void, symbol: *const c_char) -> i32 {
    ffi_guard(handle as *mut Engine, |engine| {
        let symbol = unsafe { cstr_to_string(symbol) }?;
        engine.request_symbol(&symbol);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn md_copy_bars(
    handle: *mut c_void,
    symbol: *const c_char,
    periodicity_seconds: i32,
    output: *mut MDBar,
    capacity: i32,
) -> i32 {
    let engine_ptr = handle as *mut Engine;
    let engine = match unsafe { (engine_ptr as *const Engine).as_ref() } {
        Some(e) => e,
        None => return -3,
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        if output.is_null() || capacity <= 0 {
            return 0i32;
        }
        let symbol = match unsafe { cstr_to_string(symbol) } {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let mut internal_buf = vec![Bar::default(); capacity as usize];
        let count = engine
            .cache
            .copy_bars(&symbol, periodicity_seconds, &mut internal_buf);

        let out_slice = unsafe { std::slice::from_raw_parts_mut(output, capacity as usize) };
        for i in 0..count {
            out_slice[i] = (&internal_buf[i]).into();
        }
        count as i32
    }));

    result.unwrap_or(-2)
}

#[no_mangle]
pub extern "C" fn md_copy_recent(
    handle: *mut c_void,
    symbol: *const c_char,
    output: *mut MDRecent,
) -> i32 {
    let engine_ptr = handle as *mut Engine;
    let engine = match unsafe { (engine_ptr as *const Engine).as_ref() } {
        Some(e) => e,
        None => return -3,
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        if output.is_null() {
            return -1;
        }
        let symbol = match unsafe { cstr_to_string(symbol) } {
            Ok(s) => s,
            Err(_) => return -1,
        };
        match engine.cache.get_recent(&symbol) {
            Some(recent) => {
                unsafe { *output = (&recent).into() };
                0
            }
            None => -1,
        }
    }));

    result.unwrap_or(-2)
}

#[no_mangle]
pub extern "C" fn md_is_backfill_complete(handle: *mut c_void, symbol: *const c_char) -> i32 {
    let engine_ptr = handle as *mut Engine;
    let engine = match unsafe { (engine_ptr as *const Engine).as_ref() } {
        Some(e) => e,
        None => return 0,
    };

    catch_unwind(AssertUnwindSafe(|| {
        let Ok(symbol) = (unsafe { cstr_to_string(symbol) }) else {
            return 0;
        };
        if engine.cache.is_backfill_complete(&symbol) {
            1
        } else {
            0
        }
    }))
    .unwrap_or(0)
}

// last_error returns a stable pointer through a thread-local cache of the latest CString.
thread_local! {
    static LAST_ERROR_CSTRING: Mutex<Option<CString>> = Mutex::new(None);
}

#[no_mangle]
pub extern "C" fn md_last_error(handle: *mut c_void) -> *const c_char {
    let engine_ptr = handle as *mut Engine;
    let engine = match unsafe { (engine_ptr as *const Engine).as_ref() } {
        Some(e) => e,
        None => return std::ptr::null(),
    };

    let msg = engine.last_error();
    LAST_ERROR_CSTRING.with(|slot| {
        let cstring = CString::new(msg).unwrap_or_default();
        let ptr = cstring.as_ptr();
        *slot.lock().unwrap() = Some(cstring);
        ptr
    })
}
