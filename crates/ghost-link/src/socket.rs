//! Frames on a WebSocket: one JSON object per text message. Shared by both
//! ends of the protocol.

use futures_util::{SinkExt, StreamExt};
use ghost_protocol::Frame;
use tokio_tungstenite::tungstenite::{Error, Message};

pub async fn send<S>(sink: &mut S, frame: &Frame) -> anyhow::Result<()>
where
    S: SinkExt<Message, Error = Error> + Unpin,
{
    sink.send(Message::text(serde_json::to_string(frame)?))
        .await?;
    Ok(())
}

/// The next frame, or `None` once the other side has closed.
pub async fn recv<S>(stream: &mut S) -> anyhow::Result<Option<Frame>>
where
    S: StreamExt<Item = Result<Message, Error>> + Unpin,
{
    loop {
        match stream.next().await {
            None | Some(Ok(Message::Close(_))) => return Ok(None),
            Some(Ok(Message::Text(text))) => {
                return Ok(Some(serde_json::from_str(text.as_str())?));
            }
            // Pings are answered by the socket itself; nothing else is ours.
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e.into()),
        }
    }
}
