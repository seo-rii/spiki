use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const MAX_FRAME_BYTES: usize = 1024 * 1024;

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

pub(crate) async fn read_frame<R>(reader: &mut BufReader<R>) -> Result<Option<Value>>
where
    R: AsyncRead + Unpin,
{
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
    if content_length > MAX_FRAME_BYTES {
        return Err(anyhow!("frame exceeds {} bytes", MAX_FRAME_BYTES));
    }
    let mut payload = vec![0; content_length];
    reader.read_exact(&mut payload).await?;
    Ok(Some(serde_json::from_slice(&payload)?))
}

pub(crate) async fn write_frame<W>(writer: &mut W, value: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(value)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(anyhow!("frame exceeds {} bytes", MAX_FRAME_BYTES));
    }
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", payload.len()).as_bytes())
        .await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tokio::io::{sink, BufReader};

    use super::{read_frame, write_frame, MAX_FRAME_BYTES};

    #[tokio::test]
    async fn read_frame_rejects_oversized_payloads() {
        let input = format!("Content-Length: {}\r\n\r\n", MAX_FRAME_BYTES + 1);
        let mut reader = BufReader::new(Cursor::new(input.into_bytes()));
        let error = read_frame(&mut reader).await.unwrap_err();
        assert!(error.to_string().contains("frame exceeds"));
    }

    #[tokio::test]
    async fn write_frame_rejects_oversized_payloads() {
        let mut writer = sink();
        let error = write_frame(
            &mut writer,
            &serde_json::json!({
                "payload": "x".repeat(MAX_FRAME_BYTES)
            }),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("frame exceeds"));
    }
}
