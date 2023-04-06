use std::time::Duration;

//==============================================================================
// Data
//==============================================================================

static mut DATA: Option<Vec<(&str, Duration)>> = None;

//==============================================================================
// Macros
//==============================================================================

#[macro_export]
macro_rules! profile {
    ($e:expr, $name:expr) => {{
        let begin = std::time::Instant::now();
        let expr_result = $e;
        let end = std::time::Instant::now();
        crate::tcpmig_profiler::data().push(($name, end - begin));
        expr_result
    }};
}

/// Merges this time interval with the last one profiled if there is one, else just acts like `profile!()`.
#[macro_export]
macro_rules! profile_merge_previous {
    ($e:expr) => {{
        let begin = std::time::Instant::now();
        let expr_result = $e;
        let end = std::time::Instant::now();
        crate::tcpmig_profiler::data().last_mut().expect("no previous value").1 += end - begin;
        expr_result
    }};
}

//==============================================================================
// Functions
//==============================================================================

pub fn data() -> &'static mut Vec<(&'static str, Duration)> {
    unsafe { DATA.as_mut().expect("tcpmig_profiler not initialised") }
}

pub fn init_profiler() {
    if let Some(..)  = unsafe { DATA.as_ref() } {
        panic!("Double initialisation of tcpmig_profiler");
    }
    unsafe { DATA = Some(Vec::new()); }
}

pub fn write_profiler_data<W: std::io::Write>(w: &mut W) -> std::io::Result<()> {
    let data: &Vec<(&str, Duration)> = data();
    for (name, datum) in data {
        write!(w, "{}: {} ns", name, datum.as_nanos())?;
    }
    Ok(())
}