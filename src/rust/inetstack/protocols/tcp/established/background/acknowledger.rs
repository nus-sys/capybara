// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use super::ControlBlock;
use crate::runtime::fail::Fail;
use ::futures::{
    future::{
        self,
        Either,
    },
    FutureExt,
};
use ::std::rc::Rc;

#[cfg(feature = "capybara-log")]
use crate::tcpmig_profiler::tcp_log;

pub async fn acknowledger<const N: usize>(cb: Rc<ControlBlock<N>>) -> Result<!, Fail> {
    loop {
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("acknowledger polled"));
        }
        // TODO: Implement TCP delayed ACKs, subject to restrictions from RFC 1122
        // - TCP should implement a delayed ACK
        // - The delay must be less than 500ms
        // - For a stream of full-sized segments, there should be an ack for every other segment.

        // TODO: Implement SACKs
        let (ack_deadline, ack_deadline_changed) = cb.get_ack_deadline();
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("1"));
        }
        futures::pin_mut!(ack_deadline_changed);
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("2"));
        }

        let ack_future = match ack_deadline {
            Some(t) => {
                #[cfg(feature = "capybara-log")]
                {
                    tcp_log(format!("3"));
                }
                Either::Left(cb.clock.wait_until(cb.clock.clone(), t).fuse())
            },
            None => {
                #[cfg(feature = "capybara-log")]
                {
                    tcp_log(format!("4"));
                }
                Either::Right(future::pending())
            },
        };
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("5"));
        }
        futures::pin_mut!(ack_future);
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("6"));
        }

        futures::select_biased! {
            _ = ack_deadline_changed => {
                #[cfg(feature = "capybara-log")]
                {
                    tcp_log(format!("7"));
                }
                continue
            },
            _ = ack_future => {
                #[cfg(feature = "capybara-log")]
                {
                    tcp_log(format!("ACK DEADLINE REACHED"));
                }
                cb.send_ack();
            },
        }
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("8"));
        }
    }
}
