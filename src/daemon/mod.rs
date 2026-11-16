// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Connection to the hide.me service.

mod client;
mod http;

pub use client::{Client, DEFAULT_SOCKET, Flow, Observation, ServiceState, Timing, Watch};
pub use http::Error;
