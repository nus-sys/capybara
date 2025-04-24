use crate::autokernel::parameters::{get_param, set_param, AutokernelParameters};
use crate::autokernel::observations::{get_obs_param, AutokernelObservations};
use crate::capy_log;

use libc::{eventfd_write, mmap, shm_open, ftruncate, MAP_FAILED, MAP_SHARED, PROT_READ, PROT_WRITE};
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;
use std::cell::RefCell;
use std::ffi::CString;
use std::mem;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::ptr;
use std::thread;
use std::time::Duration;

//==============================================================================
// Constants and Structs
//==============================================================================

const SHM_NAME: &str = "/autokernel_feedback_shm";
const SHM_SIZE: usize = std::mem::size_of::<CombinedFeedback>();
const SOCKET_PATH: &str = "/tmp/eventfd_socket";

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CombinedFeedback {
    pub parameters: AutokernelParameters,
    pub observations: AutokernelObservations,
}

//==============================================================================
// Thread-local state
//==============================================================================

thread_local! {
    static SHM_PTR: RefCell<Option<*mut CombinedFeedback>> = RefCell::new(None);
    static EVENTFD: RefCell<Option<i32>> = RefCell::new(None);
}

//==============================================================================
// Initialization
//==============================================================================

fn init_shared_memory() -> *mut CombinedFeedback {
    let shm_name = CString::new(SHM_NAME).expect("CString::new failed");

    let fd = unsafe {
        shm_open(
            shm_name.as_ptr(),
            OFlag::O_CREAT.bits() | OFlag::O_RDWR.bits(),
            0o666, // <-- Make it world-readable and writable
        )
    };
    if fd < 0 {
        panic!("Failed to open shared memory");
    }

    let res = unsafe { ftruncate(fd, SHM_SIZE as i64) };
    if res < 0 {
        panic!("Failed to resize shared memory");
    }

    let ptr = unsafe {
        mmap(
            ptr::null_mut(),
            SHM_SIZE,
            PROT_READ | PROT_WRITE,  // <-- allow controller to write too
            MAP_SHARED,
            fd,
            0,
        )
    };

    if ptr == MAP_FAILED {
        panic!("Failed to mmap shared memory");
    }

    ptr as *mut CombinedFeedback
}

fn recv_fd(socket: &UnixStream) -> RawFd {
    let mut buf = [0u8; 1];
    let mut cmsg_buf = [0u8; 32];

    let iov = [libc::iovec {
        iov_base: buf.as_mut_ptr() as *mut _,
        iov_len: buf.len(),
    }];

    let mut msg = libc::msghdr {
        msg_name: ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: iov.as_ptr() as *mut _,
        msg_iovlen: iov.len(),
        msg_control: cmsg_buf.as_mut_ptr() as *mut _,
        msg_controllen: cmsg_buf.len(),
        msg_flags: 0,
    };

    unsafe {
        let ret = libc::recvmsg(socket.as_raw_fd(), &mut msg, 0);
        if ret == -1 {
            panic!("recvmsg failed");
        }

        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            panic!("No file descriptor received");
        }

        *(libc::CMSG_DATA(cmsg) as *const RawFd)
    }
}

fn init_eventfd() -> i32 {
    let sock = UnixStream::connect(SOCKET_PATH).expect("Failed to connect to eventfd socket");
    recv_fd(&sock)
}

pub fn init_feedback() {
    SHM_PTR.with(|cell| {
        *cell.borrow_mut() = Some(init_shared_memory());
    });

    EVENTFD.with(|cell| {
        *cell.borrow_mut() = Some(init_eventfd());
    });
}

//==============================================================================
// Feedback Update
//==============================================================================
use libc::eventfd_read;

pub fn write_feedback_and_notify() {
    SHM_PTR.with(|shm_cell| {
        if let Some(ptr) = *shm_cell.borrow() {
            EVENTFD.with(|efd_cell| {
                if let Some(efd) = *efd_cell.borrow() {
                    let mut val: u64 = 0;
                    let res = unsafe { eventfd_read(efd, &mut val) };
                    if res < 0 {
                        let err = std::io::Error::last_os_error();
                        if err.raw_os_error() == Some(libc::EAGAIN) {
                            // No controller write, skip
                            return;
                        } else {
                            panic!("Failed to read from eventfd: {}", err);
                        }
                    }

                    // Controller wrote something
                    let controller_snapshot = unsafe { *ptr };

                    // Compare with current thread-local parameter
                    let current_batch_size = get_param(|p| p.receive_batch_size);
                    let controller_batch_size = controller_snapshot.parameters.receive_batch_size;

                    if current_batch_size != controller_batch_size {
                        capy_log!("  Controller wrote (via eventfd):");
                        capy_log!("  controller.receive_batch_size = {}", controller_batch_size);
                    }

                    // Apply controller-written values into thread-local parameters
                    set_param(|p| *p = controller_snapshot.parameters);

                    // Compose the new snapshot (with updated thread-local values + latest observations)
                    let new_snapshot = CombinedFeedback {
                        parameters: get_param(|p| *p),
                        observations: get_obs_param(|o| *o),
                    };

                    // Write updated snapshot to shared memory
                    unsafe {
                        *ptr = new_snapshot;
                    }

                    // Notify controller via eventfd
                    if unsafe { eventfd_write(efd, 1) } < 0 {
                        panic!("Failed to write back to eventfd");
                    }

                    // capy_log!("Wrote back snapshot + notified controller.\n");
                } else {
                    panic!("EVENTFD not initialized");
                }
            });
        } else {
            panic!("SHM_PTR not initialized");
        }
    });
}