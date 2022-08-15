use libc::{c_char, c_void};

use std::ptr;
use std::thread::sleep;
use std::time::Duration;

#[link(name = "xdo")]
extern "C" {
    fn xdo_free(xdo: *const c_void);
    fn xdo_new(display: *const c_char) -> *const c_void;
}

pub fn wait_for_x11() {
    if cfg!(target_os = "linux") {
        unsafe {
            let mut xdo = ptr::null();

            while xdo == ptr::null() {
                xdo = xdo_new(ptr::null());

                sleep(Duration::from_secs(1));
            }

            xdo_free(xdo);
        }
    }
}
