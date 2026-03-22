use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

pub(crate) fn id_to_string(id: &Value) -> Result<String> {
    if let Some(value) = id.as_str() {
        return Ok(value.to_string());
    }
    if let Some(value) = id.as_i64() {
        return Ok(value.to_string());
    }
    if let Some(value) = id.as_u64() {
        return Ok(value.to_string());
    }
    Err(anyhow!("unsupported response id"))
}

pub(crate) async fn read_frame(reader: &mut BufReader<OwnedReadHalf>) -> Result<Option<Value>> {
    let mut content_length = None;

    loop {
        let mut line = Vec::new();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            return Ok(None);
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        let line = String::from_utf8(line)?;
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("Content-Length") {
                content_length = Some(value.trim().parse::<usize>()?);
            }
        }
    }

    let content_length = content_length.context("missing Content-Length header")?;
    let mut payload = vec![0; content_length];
    reader.read_exact(&mut payload).await?;
    Ok(Some(serde_json::from_slice(&payload)?))
}

pub(crate) async fn write_frame(writer: &mut OwnedWriteHalf, value: &Value) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", payload.len()).as_bytes())
        .await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}
