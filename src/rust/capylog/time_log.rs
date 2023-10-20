use chrono::NaiveTime;

//==============================================================================
// Constants
//==============================================================================

#[allow(unused)]
pub(crate) const CYAN: &'static str = "\x1B[36m";
#[allow(unused)]
pub(crate) const CLEAR: &'static str = "\x1B[0m";

//==============================================================================
// Data
//==============================================================================

#[allow(unused)]
static mut DATA: Option<Vec<(NaiveTime, &'static str)>> = None;

//==============================================================================
// Macros
//==============================================================================

macro_rules! __capy_time_log {
    ($($arg:tt)*) => {
        $crate::capylog::time_log::__push_time_log($($arg)*);
    };
}

macro_rules! __capy_time_log_dump {
    ($dump:expr) => {
        $crate::capylog::time_log::__write_time_log_data($dump).expect("capy_time_log_dump failed");
    };
}

#[allow(unused)]
pub(crate) use __capy_time_log;
#[allow(unused)]
pub(crate) use __capy_time_log_dump;

//==============================================================================
// Structures
//==============================================================================



//==============================================================================
// Standard Library Trait Implementations
//==============================================================================



//==============================================================================
// Implementations
//==============================================================================



//==============================================================================
// Functions
//==============================================================================

#[allow(unused)]
pub(crate) fn __write_time_log_data<W: std::io::Write>(w: &mut W) -> std::io::Result<()> {
    eprintln!("\n[CAPYLOG] dumping time log data");
    let data = data();
    for (time, msg) in data.iter() {
        write!(w, "{}: {}\n", time, msg)?;
    }
    Ok(())
}

#[allow(unused)]
pub(crate) fn __push_time_log(msg: &'static str) {
    let data = data();
    if data.len() == data.capacity() {
        eprintln!("[CAPYLOG-WARN] Time log allocation");
    }
    data.push((chrono::Local::now().time(), msg));
}

#[allow(unused)]
fn data() -> &'static mut Vec<(NaiveTime, &'static str)> {
    unsafe { DATA.as_mut().expect("capy-time-log not initialised") }
}

#[allow(unused)]
pub(super) fn init() {
    unsafe {
        DATA = Some(Vec::with_capacity(1024));
    }

    eprintln!("[CAPYLOG] capy_time_log is on");
}