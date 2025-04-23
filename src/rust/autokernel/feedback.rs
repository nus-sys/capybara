use crate::autokernel::parameters::{get_param, AutokernelParameters};
use crate::autokernel::observations::{get_obs_param, AutokernelObservations};

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

pub fn write_feedback_and_notify() {
    let snapshot = CombinedFeedback {
        parameters: get_param(|p| *p),
        observations: get_obs_param(|o| *o),
    };

    SHM_PTR.with(|shm_cell| {
        if let Some(ptr) = *shm_cell.borrow() {
            unsafe {
                *ptr = snapshot;
            }

            EVENTFD.with(|efd_cell| {
                if let Some(efd) = *efd_cell.borrow() {
                    let res = unsafe { eventfd_write(efd, 1) };
                    if res < 0 {
                        panic!("Failed to notify eventfd");
                    }
                } else {
                    panic!("EVENTFD not initialized");
                }
            });
        } else {
            panic!("SHM_PTR not initialized");
        }
    });
}
