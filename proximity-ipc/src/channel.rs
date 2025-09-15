//! Wrappers around crossbeam channels to bridge between the IPC framework and the protocol.
//!
//! This "glue code" does some complex logical mapping between channel types given the required trait bounds, but allwos
//! a complete logical separation of the protocol from the io loops.

use std::{marker::PhantomData, time::Duration};

use anyhow::Result;
use crossbeam::channel::{Receiver as CrossbeamReceiver, Sender as CrossbeamSender};
pub use crossbeam::channel::{RecvError, RecvTimeoutError, SendError, TryRecvError, TrySendError};
use network::connection::MsgToken;
use protocol::{Request, Response};

use crate::codec::{IpcRequest, IpcResponse};

/// Wrapper for crossbeam receivers to also do type translation.
///
/// Will translate an incoming message from the underlying channel `Input` into a message of type `Output` for
/// consumption downstream. During this process, the wrapped [`MsgToken`] is preserved. Note that unlike senders, this
/// is not generic over a token type because both client and server receive [`MsgToken`] types.
pub struct Receiver<Input, Output> {
    inner: CrossbeamReceiver<(MsgToken, Input)>,
    _output: PhantomData<Output>,
}

impl<Input, Output: From<Input>> Receiver<Input, Output> {
    pub fn recv(&self) -> Result<(MsgToken, Output), RecvError> {
        self.inner.recv().map(msg_map)
    }

    pub fn try_recv(&self) -> Result<(MsgToken, Output), TryRecvError> {
        self.inner.try_recv().map(msg_map)
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<(MsgToken, Output), RecvTimeoutError> {
        self.inner.recv_timeout(timeout).map(msg_map)
    }
}

/// Wrapper for crossbeam senders to also do type translation.
///
/// Will translate an input with the generic `Input` to an output type of `Output`, then dispatch this output type on
/// the inner channel. During this process the token `Token` is passed through unchanged. [`Sender`] requires a generic
/// on the token as clients and servers have different sender requirements.
pub struct Sender<Ouput, Input, Token = MsgToken> {
    inner: CrossbeamSender<(Token, Ouput)>,
    _input: PhantomData<(Token, Input)>,
}

// NOTE: Unsure why we need the manual implementation of clone, why can't it be derived?
impl<T, U, Token> Clone for Sender<T, U, Token> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _input: PhantomData,
        }
    }
}

// TODO: Consider if some component of this from flow should be pushed into the network crate
impl<Output: From<Input>, Input: From<Output>, Token> Sender<Output, Input, Token> {
    pub fn send(&self, msg: (Token, Input)) -> Result<(), SendError<(Token, Input)>> {
        self.inner
            .send(msg_map(msg))
            .map_err(|err| SendError(msg_map(err.0)))
    }

    pub fn try_send(&self, msg: (Token, Input)) -> Result<(), TrySendError<(Token, Input)>> {
        self.inner.try_send(msg_map(msg)).map_err(|err| match err {
            TrySendError::Full(msg) => TrySendError::Full(msg_map(msg)),
            TrySendError::Disconnected(msg) => TrySendError::Disconnected(msg_map(msg)),
        })
    }
}

#[inline]
fn msg_map<T, U: From<T>, Token>((token, msg): (Token, T)) -> (Token, U) {
    (token, U::from(msg))
}

pub type RequestSender<Token = MsgToken> = Sender<IpcRequest, Request, Token>;
pub type RequestReceiver = Receiver<IpcRequest, Request>;
pub type ResponseSender<Token = MsgToken> = Sender<IpcResponse, Response, Token>;
pub type ResponseReceiver = Receiver<IpcResponse, Response>;

/// Set of channels for communicating between the server's io loop and business logic.
pub struct ServerChannels {
    /// The server's io_loop needs a raw crossbeam channel and an IoCodec implementor.
    pub request_tx: CrossbeamSender<(MsgToken, IpcRequest)>,

    /// The server's business logic needs the protocol's request for ease of receiving in handlers, and doesn't mind
    /// being wrapped.
    pub request_rx: RequestReceiver,

    /// The server's business logic needs the protocol's response for ease of sending to the io_loop, and doesn't mind
    /// being wrapped.
    pub response_tx: ResponseSender,

    /// The server's io loop needs a raw crossbeam channel and an IoCodec implementor.
    pub response_rx: CrossbeamReceiver<(MsgToken, IpcResponse)>,
}

impl ServerChannels {
    /// Generate appropriate server-oriented channels for encoded/decoded messages.
    ///
    /// Server channels have request senders and response receivers as vanilla crossbeam to send to the io loop, and wrapped
    /// other sides to convert to protocol requests and responses in the application.
    pub fn new(request_capacity: usize, response_capcity: usize) -> ServerChannels {
        let (request_tx, request_rx_raw) = crossbeam::channel::bounded(request_capacity);
        let (response_tx_raw, response_rx) = crossbeam::channel::bounded(response_capcity);

        let request_rx = Receiver {
            inner: request_rx_raw,
            _output: PhantomData,
        };
        let response_tx = Sender {
            inner: response_tx_raw,
            _input: PhantomData,
        };

        ServerChannels {
            request_tx,
            request_rx,
            response_tx,
            response_rx,
        }
    }
}

/// Generate appropriate client-oriented channels for encoded and decoded messages.
///
/// Client channels have request receivers and response senders as vanilla crossbeam to send to the io loop, and
/// werapped other sides to conver to protocol requests and responses in the application.
///
/// Additionally, client channels have no use for the for MsgToken, as we are only interested in the request id for
/// message tracking - there is only one connection, so the connection id is irrelevant.
pub struct ClientChannels {
    /// The client's business logic needs the protocol's request for ease of use, and doesn't mind being wrapped.
    pub request_tx: RequestSender<u32>,

    /// The client's io_loop needs a raw crossbeam channel containing just the req is and an IoCodec implementor.
    pub request_rx: CrossbeamReceiver<(u32, IpcRequest)>,

    /// The client's io_loop needs a raw crossbeam channel containing just the req id and an IoCodec implementor.
    pub response_tx: CrossbeamSender<(MsgToken, IpcResponse)>,

    /// The client's  business logic needs the protocol's response for ease of use and doesn't mind being wrapped.
    pub response_rx: ResponseReceiver,
}

impl ClientChannels {
    pub fn new(request_capacity: usize, response_capcity: usize) -> ClientChannels {
        let (request_tx_raw, request_rx) = crossbeam::channel::bounded(request_capacity);
        let (response_tx, response_rx_raw) = crossbeam::channel::bounded(response_capcity);

        let request_tx = Sender {
            inner: request_tx_raw,
            _input: PhantomData,
        };
        let response_rx = Receiver {
            inner: response_rx_raw,
            _output: PhantomData,
        };

        ClientChannels {
            request_tx,
            request_rx,
            response_tx,
            response_rx,
        }
    }
}
