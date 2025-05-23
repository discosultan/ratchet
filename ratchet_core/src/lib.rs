// Copyright 2015-2021 Swim Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Ratchet's core implementation. See the Ratchet crate for usage.

#![deny(
    missing_docs,
    missing_copy_implementations,
    missing_debug_implementations,
    trivial_numeric_casts,
    unstable_features,
    unused_must_use,
    unused_mut,
    unused_imports,
    unused_import_braces
)]

extern crate core;

#[cfg(test)]
mod test_fixture;

mod builder;
mod errors;
mod ext;
mod framed;
mod handshake;
mod protocol;
mod ws;

/// Split WebSocket implementation.
#[cfg(feature = "split")]
mod split;
#[cfg(feature = "split")]
pub use split::{Receiver, ReuniteError, Sender};

#[allow(missing_docs)]
#[cfg(feature = "fixture")]
pub mod fixture {
    pub use super::protocol::write_text_frame_header;
}

pub use builder::{WebSocketClientBuilder, WebSocketServerBuilder};
pub use errors::*;
pub use ext::{NoExt, NoExtDecoder, NoExtEncoder, NoExtProvider};
pub use handshake::{
    accept, accept_with, subscribe, subscribe_with, SubprotocolRegistry, TryIntoRequest,
    UpgradedClient, UpgradedServer, WebSocketResponse, WebSocketUpgrader,
};
pub use protocol::{
    CloseCode, CloseReason, Message, MessageType, PayloadType, Role, WebSocketConfig,
};
pub use ws::{CloseState, WebSocket};

use tokio::io::{AsyncRead, AsyncWrite};

pub(crate) type Request = http::Request<()>;

/// A stream representing a WebSocket connection.
pub trait WebSocketStream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<S> WebSocketStream for S where S: AsyncRead + AsyncWrite + Send + Unpin {}

/// Provides utilities for handling WebSocket handshakes on the server side.
///
/// This module includes the necessary components to parse, negotiate, and respond to WebSocket
/// connection upgrade requests from clients.
///
/// It should generally not be required unless integrating Ratchet into other libraries.
pub mod server {
    pub use crate::handshake::{
        build_response, build_response_headers, handshake, parse_request_parts,
        response_from_headers, validate_method_and_version, UpgradeRequest, UpgradeRequestParts,
        UpgradeResponseParts,
    };
}
