use std::time::{Duration, Instant};

//==============================================================================
// Data
//==============================================================================

static mut DATA: Option<Vec<(&str, Duration)>> = None;

//==============================================================================
// Macros
//==============================================================================

#[macro_export]
macro_rules! tcpmig_profile {
    ($name:expr) => {
        let __tcpmig_profiler_dropped_object__ = crate::tcpmig_profiler::DroppedObject::begin($name);
    };
}

/// Merges this time interval with the last one profiled, ensuring that the name is the same.
#[macro_export]
macro_rules! tcpmig_profile_merge_previous {
    ($name:expr) => {{
        let __tcpmig_profiler_dropped_object__ = crate::tcpmig_profiler::MergeDroppedObject::begin($name);
    }};
}

//==============================================================================
// Structures
//==============================================================================

pub struct DroppedObject {
    name: &'static str,
    begin: Instant,
}

pub struct MergeDroppedObject {
    begin: Instant,
}

//==============================================================================
// Standard Library Trait Implementations
//==============================================================================

impl Drop for DroppedObject {
    fn drop(&mut self) {
        let end = Instant::now();
        data().push((self.name, end - self.begin));
    }
}

impl Drop for MergeDroppedObject {
    fn drop(&mut self) {
        let end = Instant::now();
        data().last_mut().expect("no previous value").1 += end - self.begin;
    }
}

//==============================================================================
// Implementations
//==============================================================================

impl DroppedObject {
    pub fn begin(name: &'static str) -> Self {
        Self {
            name,
            begin: Instant::now()
        }
    }
}

impl MergeDroppedObject {
    pub fn begin(name: &'static str) -> Self {
        match data().last() {
            None => panic!("tcpmig_profiler: no previous value"),
            Some(&(prev, _)) if prev != name => panic!("tcpmig_profiler: expected \"{}\", found \"{}\"", name, prev),
            _ => (),
        }

        Self {
            begin: Instant::now()
        }
    }
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
        write!(w, "{}: {} ns\n", name, datum.as_nanos())?;
    }
    Ok(())
}