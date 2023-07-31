// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use demikernel::{demi_sgarray_t, runtime::types::demi_opcode_t};
use ::demikernel::{
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};

#[cfg(feature = "capybara-log")]
use ::demikernel::tcpmig_profiler::tcp_log;

use std::{collections::HashMap, time::Duration};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;
const ROOT: &str = "/var/www/demo";

use std::sync::atomic::{AtomicBool, Ordering};
use ctrlc;
use std::sync::Arc;

const BUFSZ: usize = 4096;

// Borrowed from Loadgen
struct Buffer {
    buf: Vec<u8>,
    head: usize,
    tail: usize,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
            buf: vec![0; BUFSZ],
            head: 0,
            tail: 0,
        }
    }

    pub fn data_size(&self) -> usize {
        self.head - self.tail
    }

    pub fn get_data(&self) -> &[u8] {
        &self.buf[self.tail..self.head]
    }

    pub fn push_data(&mut self, size: usize) {
        self.head += size;
        assert!(self.head <= self.buf.len());
    }

    pub fn pull_data(&mut self, size: usize) {
        assert!(size <= self.data_size());
        self.tail += size;
    }

    pub fn get_empty_buf(&mut self) -> &mut [u8] {
        &mut self.buf[self.head..]
    }

    pub fn try_shrink(&mut self) -> Result<()> {
        if self.data_size() == 0 {
            self.head = 0;
            self.tail = 0;
            return Ok(());
        }

        if self.head < self.buf.len() {
            return Ok(());
        }

        if self.data_size() == self.buf.len() {
            panic!("Need larger buffer for HTTP messages");
        }

        self.buf.copy_within(self.tail..self.head, 0);
        self.head = self.data_size();
        self.tail = 0;
        Ok(())
    }
}

fn respond_to_request(libos: &mut LibOS, qd: QDesc, data: &[u8]) -> QToken {
    // let data_str = std::str::from_utf8(data).unwrap();
    // let data_str = String::from_utf8_lossy(data);
    let data_str = unsafe { std::str::from_utf8_unchecked(&data[..]) };

    let mut file_name = data_str
            .split_whitespace()
            .nth(1)
            .and_then(|file_path| {
                let mut path_parts = file_path.split('/');
                path_parts.next().and_then(|_| path_parts.next())
            })
            .unwrap_or("index.html");
    if file_name == "" {
        file_name = "index.html";
    }
    let full_path = format!("{}/{}", ROOT, file_name);
    
    let response = match std::fs::read_to_string(full_path.as_str()) {
        Ok(contents) => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", contents.len(), contents),
        Err(_) => format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n"),
    };
    #[cfg(feature = "capybara-log")]
    {
        tcp_log(format!("PUSH: {}", response.lines().next().unwrap_or("")));
    }

    let sga = mksga(libos, response.as_bytes()).unwrap();
    libos.push(qd, &sga).expect("push success")
}

#[inline(always)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn push_data_and_run(libos: &mut LibOS, qd: QDesc, buffer: &mut Buffer, data: &[u8], qts: &mut Vec<QToken>) -> usize {

    // fast path: no previous data in the stream and this request contains exactly one HTTP request
    if buffer.data_size() == 0 {
        if find_subsequence(data, b"\r\n\r\n").unwrap_or(data.len()) == data.len() - 4 {
            let resp_qt = respond_to_request(libos, qd, data);
            qts.push(resp_qt);
            return 1;
        }
    }
    println!("**********************CHECK buffer data size : {} ************************\n", buffer.data_size());
    // Copy new data into buffer
    buffer.get_empty_buf()[..data.len()].copy_from_slice(data);
    buffer.push_data(data.len());

    let mut sent = 0;

    loop {
        let dbuf = buffer.get_data();
        match find_subsequence(dbuf, b"\r\n\r\n") {
            Some(idx) => {
                let resp_qt = respond_to_request(libos, qd, &dbuf[..idx + 4]);
                qts.push(resp_qt);
                buffer.pull_data(idx + 4);
                buffer.try_shrink().unwrap();
                sent += 1;
            }
            None => {
                return sent;
            }
        }
    }
}


//======================================================================================================================
// server()
//======================================================================================================================


struct ConnectionState {
    pushing: usize,
    pop_qt: QToken,
    buffer: Buffer,

}

fn server(local: SocketAddrV4) -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    let sockqd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0).expect("created socket");

    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, ConnectionState> = HashMap::new();

    qts.push(libos.accept(sockqd).expect("accept"));

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let result = match libos.wait_any_multiple(&qts, Some(Duration::from_micros(1))){
            Ok(wait_result) => Some(wait_result),
            Err(e) if e.errno == libc::ETIMEDOUT => None,
            Err(e) => panic!("result failed: {:?}", e.cause),
        };

        if let Some(completed_results) = result {
            #[cfg(feature = "capybara-log")]
            {
                tcp_log(format!("\n\n======= OS: I/O operations have been completed, take the results! ======="));
            }
            let indices_to_remove: Vec<usize> = completed_results.iter().map(|(index, _)| *index).collect();
            #[cfg(feature = "capybara-log")]
            {
                tcp_log(format!("\n\n1, indicies_to_remove: {:?}", indices_to_remove));
            }
            qts = qts.iter().enumerate().filter(|(i, _)| !indices_to_remove.contains(i)).map(|(_, qt)| *qt).collect(); //HERE!
            #[cfg(feature = "capybara-log")]
            {
                tcp_log(format!("\n\n2"));
            }
            for (_, completed_result) in completed_results {
                match completed_result.qr_opcode {
                    demi_opcode_t::DEMI_OPC_ACCEPT => {
                        #[cfg(feature = "capybara-log")]
                        {
                            tcp_log(format!("ACCEPT complete ==> request POP and ACCEPT"));
                        }
                        // Pop from new_qd
                        let new_qd = unsafe { completed_result.qr_value.ares.qd.into() };
                        let pop_qt = libos.pop(new_qd, None).expect("pop qt");
                        qts.push(pop_qt);
                        connstate.insert(new_qd, ConnectionState {
                            pushing: 0,
                            pop_qt,
                            buffer: Buffer::new(),
                        });
    
                        // Re-arm accept
                        qts.push(libos.accept(completed_result.qr_qd.into()).expect("accept qtoken"));
                    },
                    demi_opcode_t::DEMI_OPC_PUSH => {
                        connstate.get_mut(&completed_result.qr_qd.into()).unwrap().pushing -= 1;
                        #[cfg(feature = "capybara-log")]
                        {
                            tcp_log(format!("PUSH complete ==> {} pushes are pending", connstate.get_mut(&completed_result.qr_qd.into()).unwrap().pushing));
                        }
                    },
                    demi_opcode_t::DEMI_OPC_POP => {
                        #[cfg(feature = "capybara-log")]
                        {
                            tcp_log(format!("POP complete ==> request PUSH and POP"));
                        }
                        let qd = completed_result.qr_qd.into();
                        let sga = unsafe { completed_result.qr_value.sga };
                        let recvbuf = unsafe { std::slice::from_raw_parts(sga.sga_segs[0].sgaseg_buf as *const u8, sga.sga_segs[0].sgaseg_len as usize) };
                        let state = connstate.get_mut(&qd).unwrap();
                        let sent = push_data_and_run(&mut libos, qd, &mut state.buffer, &recvbuf, &mut qts);
                        state.pushing += sent;
                        // queue next pop
                        let pop_qt = libos.pop(qd, None).expect("pop qt");
                        qts.push(pop_qt);
                        state.pop_qt = pop_qt;
                    },
                    _ => {
                        panic!("Unexpected op: RESULT: {:?}", completed_result.qr_opcode);
                    },
                }
            }
            #[cfg(feature = "capybara-log")]
            {
                tcp_log(format!("******* APP: Okay, handled the results! *******"));
            }
        }

        #[cfg(feature = "tcp-migration")]
        {
            let mut qd_to_remove = None;

            for (qd, state) in connstate.iter() {
                // Can't migrate a connection with outstanding TX or partially processed HTTP requests in the TCP stream
                if state.pushing > 0 || state.buffer.data_size() > 0 {
                    continue;
                }
                match libos.notify_migration_safety(*qd) {
                    Ok(true) => {
                        let index = qts.iter().position(|&qt| qt == state.pop_qt).expect("`pop_qt` should be in `qts`");
                        qts.swap_remove(index);
                        qd_to_remove = Some(*qd);
                        break;
                    },
                    Err(e) => panic!("notify migration safety failed: {:?}", e.cause),
                    _ => (),
                };
            }

            if let Some(qd) = qd_to_remove {
                connstate.remove(&qd);
            }
        }
    }

    eprintln!("server stopping");

    // loop {}

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    #[cfg(feature = "tcp-migration-profiler")]
    demikernel::tcpmig_profiler::write_profiler_data(&mut std::io::stdout()).unwrap();

    // TODO: close socket when we get close working properly in catnip.
    Ok(())
}

//======================================================================================================================
// usage()
//======================================================================================================================

/// Prints program usage and exits.
fn usage(program_name: &String) {
    println!("Usage: {} address\n", program_name);
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    #[cfg(feature = "capybara-log")]
    {
        tcp_log(format!("*** CAPYBARA LOGGING IS ON ***"));
    }
    // logging::initialize();

    #[cfg(feature = "tcp-migration-profiler")]
    demikernel::tcpmig_profiler::init_profiler();

    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}

// Makes a scatter-gather array.
fn mksga(libos: &mut LibOS, bytes: &[u8]) -> Result<demi_sgarray_t> {
    // Allocate scatter-gather array.
    let sga: demi_sgarray_t = match libos.sgaalloc(bytes.len()) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that scatter-gather array has the requested size.
    // If error, free scatter-gather array.
    if sga.sga_segs[0].sgaseg_len as usize != bytes.len() {
        freesga(libos, sga);
        let seglen: usize = sga.sga_segs[0].sgaseg_len as usize;
        anyhow::bail!(
            "failed to allocate scatter-gather array: expected size={:?} allocated size={:?}",
            bytes.len(),
            seglen
        );
    }

    // Fill in scatter-gather array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    slice.copy_from_slice(bytes);

    Ok(sga)
}

/// Free scatter-gather array and warn on error.
fn freesga(libos: &mut LibOS, sga: demi_sgarray_t) {
    if let Err(e) = libos.sgafree(sga) {
        log::error!("sgafree() failed (error={:?})", e);
        log::warn!("leaking sga");
    }
}