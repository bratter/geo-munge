use anyhow::Result;
use network::IoCodec;
use protocol::{Request, Response};

/// Newtype wrapper for Request to enable IoCodec implementation.
#[derive(Debug)]
pub struct IpcRequest(Request);

impl IoCodec for IpcRequest {
    fn decode_from_slice(buf: &[u8]) -> Result<Self> {
        let config = bincode::config::standard();
        let (result, _) = bincode::decode_from_slice::<Request, _>(&buf, config)?;

        Ok(Self(result))
    }

    fn encode_to_vec(&self) -> Result<Vec<u8>> {
        let config = bincode::config::standard();
        let bytes = bincode::encode_to_vec(&self.0, config)?;

        Ok(bytes)
    }
}

impl From<IpcRequest> for Request {
    fn from(value: IpcRequest) -> Self {
        value.0
    }
}

impl From<Request> for IpcRequest {
    fn from(value: Request) -> Self {
        IpcRequest(value)
    }
}

/// Newtype wrapper for Responsee to enable IoCodec implementation.
#[derive(Debug)]
pub struct IpcResponse(pub Response);

impl IoCodec for IpcResponse {
    fn decode_from_slice(buf: &[u8]) -> Result<Self> {
        let config = bincode::config::standard();
        let (result, _) = bincode::decode_from_slice::<Response, _>(&buf, config)?;

        Ok(Self(result))
    }

    fn encode_to_vec(&self) -> Result<Vec<u8>> {
        let config = bincode::config::standard();
        let bytes = bincode::encode_to_vec(&self.0, config)?;

        Ok(bytes)
    }
}

impl From<IpcResponse> for Response {
    fn from(value: IpcResponse) -> Self {
        value.0
    }
}

impl From<Response> for IpcResponse {
    fn from(value: Response) -> Self {
        IpcResponse(value)
    }
}
