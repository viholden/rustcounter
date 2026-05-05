// SPDX-License-Identifier: GPL-2.0
//! rustcounter: an atomic counter character device.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use kernel::{
    fs::{File, Kiocb},
    iov::{IovIterDest, IovIterSource},
    miscdevice::{MiscDevice, MiscDeviceOptions, MiscDeviceRegistration},
    prelude::*,
    str::CString,
};

module! {
    type: RustCounter,
    name: "rustcounter",
    description: "rustcounter — an atomic counter character device",
    license: "GPL",
}

static COUNT: AtomicU64 = AtomicU64::new(0);
static CONSUMED: AtomicBool = AtomicBool::new(false);

#[pin_data]
struct RustCounter {
    #[pin]
    _miscdev: MiscDeviceRegistration<RustCounterDevice>,
}

impl kernel::InPlaceModule for RustCounter {
    fn init(_module: &'static ThisModule) -> impl PinInit<Self, Error> {
        pr_info!("module loaded\n");
        let opts = MiscDeviceOptions { name: c"rustcounter" };
        try_pin_init!(Self {
            _miscdev <- MiscDeviceRegistration::register(opts),
        })
    }
}

struct RustCounterDevice;

#[vtable]
impl MiscDevice for RustCounterDevice {
    type Ptr = Pin<KBox<Self>>;

    fn open(_file: &File, _misc: &MiscDeviceRegistration<Self>) -> Result<Pin<KBox<Self>>> {
        Ok(KBox::new(RustCounterDevice, GFP_KERNEL).map(KBox::into_pin)?)
    }

    fn write_iter(mut kiocb: Kiocb<'_, Self::Ptr>, iov: &mut IovIterSource<'_>) -> Result<usize> {
        // Drain the user's bytes so the syscall completes cleanly
        let mut sink: KVec<u8> = KVec::new();
        let n = iov.copy_from_iter_vec(&mut sink, GFP_KERNEL)?;

        let new_count = COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        CONSUMED.store(false, Ordering::SeqCst);
        *kiocb.ki_pos_mut() = 0;

        pr_info!("incremented to {new_count}\n");
        Ok(n)
    }

    fn read_iter(mut kiocb: Kiocb<'_, Self::Ptr>, iov: &mut IovIterDest<'_>) -> Result<usize> {
        if CONSUMED.swap(true, Ordering::SeqCst) {
            return Ok(0);
        }
        let n = COUNT.load(Ordering::SeqCst);
        let formatted = CString::try_from_fmt(fmt!("{n}\n"))?;
        let bytes = formatted.to_bytes();
        let written = iov.simple_read_from_buffer(kiocb.ki_pos_mut(), bytes)?;
        Ok(written)
    }
}
