#![no_std]

extern crate alloc;

use alloc::{
    alloc::{alloc, dealloc, realloc, Layout},
    string::{String, ToString},
};
use serde_json::json;

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[unsafe(no_mangle)]
unsafe extern "C" fn memcmp(left: *const u8, right: *const u8, length: usize) -> i32 {
    for index in 0..length {
        let left = left.add(index).read();
        let right = right.add(index).read();
        if left != right {
            return i32::from(left) - i32::from(right);
        }
    }
    0
}

#[unsafe(export_name = "cabi_realloc")]
unsafe extern "C" fn canonical_realloc(
    old_ptr: *mut u8,
    old_len: usize,
    align: usize,
    new_len: usize,
) -> *mut u8 {
    if old_len == 0 {
        if new_len == 0 {
            return align as *mut u8;
        }
        let layout = Layout::from_size_align_unchecked(new_len, align);
        let ptr = alloc(layout);
        if ptr.is_null() {
            core::arch::wasm32::unreachable();
        }
        return ptr;
    }
    let layout = Layout::from_size_align_unchecked(old_len, align);
    if new_len == 0 {
        dealloc(old_ptr, layout);
        return align as *mut u8;
    }
    let ptr = realloc(old_ptr, layout, new_len);
    if ptr.is_null() {
        core::arch::wasm32::unreachable();
    }
    ptr
}

wit_bindgen::generate!({
    path: "../../wit/yaqmc-provider",
    world: "provider-account",
});

const PROVIDER_ID: &str = "org.yaqmc.providers.kugou";
const PLATFORM: &str = "kugou";
const PROVIDER_MODE: &str = match option_env!("YAQMC_PROVIDER_MODE") {
    Some(value) => value,
    None => "isolated",
};
const BACKEND_URL: &str = match option_env!("YAQMC_PROVIDER_BACKEND_URL") {
    Some(value) => value,
    None => "http://127.0.0.1:43821/v1",
};

struct KugouProvider;

impl Guest for KugouProvider {
    fn invoke(
        capability: String,
        operation: String,
        payload_json: String,
    ) -> Result<String, String> {
        yaqmc_provider_guest_core::dispatch_with_mode(
            PROVIDER_ID,
            PLATFORM,
            &capability,
            &operation,
            &payload_json,
            PROVIDER_MODE,
            |body| {
                let request = json!({
                    "method": "POST",
                    "url": BACKEND_URL,
                    "headers": {"content-type": "application/json"},
                    "body": body
                });
                yaqmc::provider::network::request(&request.to_string())
            },
        )
    }
}

export!(KugouProvider);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}
