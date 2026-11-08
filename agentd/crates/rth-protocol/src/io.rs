

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

use crate::MAX_FRAME;

#[derive(Debug)]
pub enum ReadError {
    Closed,
    TooLarge,
    Io(std::io::Error),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Closed => write!(f, "connection closed"),
            ReadError::TooLarge => write!(f, "frame larger than {MAX_FRAME} bytes"),
            ReadError::Io(e) => write!(f, "{e}"),
        }
    }
}

pub async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>) -> Result<String, ReadError> {
    let room = (MAX_FRAME + 1).saturating_sub(buf.len()) as u64;
    let n = reader.take(room).read_until(b'\n', buf).await.map_err(ReadError::Io)?;
    if buf.last() != Some(&b'\n') {
        let too_large = buf.len() > MAX_FRAME;
        buf.clear();
        return Err(if too_large { ReadError::TooLarge } else { ReadError::Closed });
    }
    if n == 0 && buf.is_empty() {
        return Err(ReadError::Closed);
    }
    buf.pop();
    String::from_utf8(std::mem::take(buf)).map_err(|e| ReadError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
}
