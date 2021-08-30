use crate::errors::{CloseError, Error, ErrorKind};
use crate::framed::{FramedIo, Item, ReadError, CONTROL_FRAME_LEN};
use crate::handshake::{exec_client_handshake, HandshakeResult, ProtocolRegistry};
use crate::protocol::{
    CloseCode, CloseReason, ControlCode, DataCode, HeaderFlags, Message, MessageType, OpCode,
    PayloadType, Role,
};
use crate::{Extension, ExtensionProvider, Request, WebSocketConfig, WebSocketStream};
use bytes::BytesMut;

const CONTROL_MAX_SIZE: usize = 125;
const CONTROL_DATA_MISMATCH: &str = "Unexpected control frame data";

pub struct WebSocket<S, E> {
    inner: WebSocketInner<S, E>,
}

impl<S, E> WebSocket<S, E>
where
    S: WebSocketStream,
    E: Extension,
{
    pub async fn read(&mut self, read_buffer: &mut BytesMut) -> Result<Message, Error> {
        self.inner.read(read_buffer).await
    }

    pub async fn write(
        &mut self,
        buf: &mut BytesMut,
        message_type: PayloadType,
    ) -> Result<(), Error> {
        self.inner.write(buf, message_type).await
    }

    pub async fn close(mut self, reason: Option<String>) -> Result<(), Error> {
        self.inner
            .framed
            .write_close(CloseReason::new(CloseCode::Normal, reason))
            .await
    }

    pub async fn send_fragmented(
        &mut self,
        buf: &mut BytesMut,
        message_type: MessageType,
        fragment_size: usize,
    ) -> Result<(), Error> {
        self.inner
            .send_fragmented(buf, message_type, fragment_size)
            .await
    }

    pub fn split(self) -> ((), ()) {
        unimplemented!()
    }
}

pub async fn client<S, E>(
    config: WebSocketConfig,
    mut stream: S,
    request: Request,
    extension: E,
) -> Result<(WebSocket<S, E::Extension>, Option<String>), Error>
where
    S: WebSocketStream,
    E: ExtensionProvider,
{
    let WebSocketConfig { max_size } = config;
    let mut read_buffer = BytesMut::new();

    let HandshakeResult {
        protocol,
        extension,
    } = exec_client_handshake(
        &mut stream,
        request,
        extension,
        ProtocolRegistry::default(),
        &mut read_buffer,
    )
    .await?;

    let socket = WebSocket {
        inner: WebSocketInner {
            framed: FramedIo::new(stream, read_buffer, Role::Client, max_size),
            _extension: extension,
            control_buffer: BytesMut::with_capacity(CONTROL_MAX_SIZE),
            closed: false,
        },
    };
    Ok((socket, protocol))
}

struct WebSocketInner<S, E> {
    framed: FramedIo<S>,
    control_buffer: BytesMut,
    _extension: E,
    closed: bool,
}

impl<S, E> WebSocketInner<S, E>
where
    S: WebSocketStream,
    E: Extension,
{
    async fn send_fragmented(
        &mut self,
        buf: &mut BytesMut,
        message_type: MessageType,
        fragment_size: usize,
    ) -> Result<(), Error> {
        if *self.closed {
            return Err(Error::with_cause(ErrorKind::Close, CloseError::Closed));
        }

        let mut chunks = buf.chunks_mut(fragment_size).peekable();
        match chunks.next() {
            Some(payload) => {
                let payload_type = match message_type {
                    MessageType::Text => DataCode::Text,
                    MessageType::Binary => DataCode::Binary,
                };

                let flags = if chunks.peek().is_none() {
                    HeaderFlags::FIN
                } else {
                    HeaderFlags::empty()
                };

                self.framed
                    .write(OpCode::DataCode(payload_type), flags, payload)
                    .await?;
            }
            None => return Ok(()),
        }

        while let Some(payload) = chunks.next() {
            let flags = if chunks.peek().is_none() {
                HeaderFlags::FIN
            } else {
                HeaderFlags::empty()
            };

            self.framed
                .write(OpCode::DataCode(DataCode::Continuation), flags, payload)
                .await?;
        }

        Ok(())
    }

    async fn read(&mut self, read_buffer: &mut BytesMut) -> Result<Message, Error> {
        let WebSocketInner {
            framed,
            closed,
            control_buffer,
            ..
        } = self;

        if *closed {
            return Err(Error::with_cause(ErrorKind::Close, CloseError::Closed));
        }

        loop {
            match framed.read_next(read_buffer).await {
                Ok(item) => match item {
                    Item::Binary => return Ok(Message::Binary),
                    Item::Text => return Ok(Message::Text),
                    Item::Ping(payload) => {
                        framed
                            .write(
                                OpCode::ControlCode(ControlCode::Pong),
                                HeaderFlags::FIN,
                                payload,
                            )
                            .await?;
                        return Ok(Message::Ping);
                    }
                    Item::Pong(payload) => {
                        if control_buffer.is_empty() {
                            continue;
                        } else {
                            return if control_buffer[..].eq(&payload[..]) {
                                Ok(Message::Pong)
                            } else {
                                self.closed = true;
                                self.framed
                                    .write_close(CloseReason {
                                        code: CloseCode::Protocol,
                                        description: Some(CONTROL_DATA_MISMATCH.to_string()),
                                    })
                                    .await?;

                                return Err(Error::with_cause(
                                    ErrorKind::Protocol,
                                    CONTROL_DATA_MISMATCH.to_string(),
                                ));
                            };
                        }
                    }
                    Item::Close((reason, payload)) => {
                        framed
                            .write(
                                OpCode::ControlCode(ControlCode::Close),
                                HeaderFlags::FIN,
                                payload,
                            )
                            .await?;

                        *closed = true;
                        return Ok(Message::Close(reason));
                    }
                },
                Err(e) => {
                    let ReadError { close_with, error } = e;
                    self.closed = true;

                    if let Some(reason) = close_with {
                        // This will only be 'None' if an IO error was encountered and so we cannot
                        // write a reason
                        self.framed.write_close(reason).await?;
                    }
                    return Err(error);
                }
            }
        }
    }

    async fn write<A>(&mut self, mut buf_ref: A, message_type: PayloadType) -> Result<(), Error>
    where
        A: AsMut<[u8]>,
    {
        if self.closed {
            return Err(Error::with_cause(ErrorKind::Close, CloseError::Closed));
        }

        let buf = buf_ref.as_mut();

        let op_code = match message_type {
            PayloadType::Text => OpCode::DataCode(DataCode::Text),
            PayloadType::Binary => OpCode::DataCode(DataCode::Binary),
            PayloadType::Ping => {
                if buf.len() > CONTROL_MAX_SIZE {
                    return Err(Error::with_cause(ErrorKind::Protocol, CONTROL_FRAME_LEN));
                } else {
                    self.control_buffer.clear();
                    self.control_buffer
                        .clone_from_slice(&buf[..CONTROL_MAX_SIZE]);
                    OpCode::ControlCode(ControlCode::Ping)
                }
            }
        };

        match self.framed.write(op_code, HeaderFlags::FIN, buf).await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.closed = true;
                Err(e.into())
            }
        }
    }
}
