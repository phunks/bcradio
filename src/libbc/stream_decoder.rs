
use anyhow::Result;
use minimp3::{Decoder, Frame};
use rodio::Source;
use std::io::Read;
use std::time::Duration;

/// This is a modified version of [rodio's Mp3Decoder](https://github.com/RustAudio/rodio/blob/55d957f8b40c59fccea4162c4b03f6dd87a7a4d9/src/decoder/mp3.rs)
/// which removes the "Seek" trait bound for streaming network audio.
///
/// Related GitHub issue:
/// https://github.com/RustAudio/rodio/issues/333
pub struct Mp3StreamDecoder<R>
where
    R: tokio::io::AsyncSeek + tokio::io::AsyncRead + std::marker::Unpin,
{
    decoder: Decoder<R>,
    current_frame: Frame,
    current_frame_offset: usize,
}

impl<R> Mp3StreamDecoder<R>
where
    R: tokio::io::AsyncSeek
        + tokio::io::AsyncRead
        + std::marker::Unpin
        + std::io::Read
{
    pub async fn new(data: R) -> Result<Self, R> {

        let mut decoder = Decoder::new(data);
        let current_frame = decoder.next_frame_future().await.unwrap();

        Ok(Self {
            decoder,
            current_frame,
            current_frame_offset: 0,
        })
    }
    pub fn into_inner(self) -> R {
        self.decoder.into_inner()
    }
}

impl<R> Source for Mp3StreamDecoder<R>
where
    R: Read + tokio::io::AsyncRead + std::marker::Unpin + tokio::io::AsyncSeek,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.current_frame.data.len())
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.current_frame.channels as _
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.current_frame.sample_rate as _
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl<R> Iterator for Mp3StreamDecoder<R>
where
    R: Read + tokio::io::AsyncRead + std::marker::Unpin + tokio::io::AsyncSeek,
{
    type Item = i16;

    #[inline]
    fn next(&mut self) -> Option<i16> {
        if self.current_frame_offset == self.current_frame.data.len() {
            match self.decoder.next_frame() {
                Ok(frame) => self.current_frame = frame,
                _ => return None,
            }
            self.current_frame_offset = 0;
        }

        let v = self.current_frame.data[self.current_frame_offset];
        self.current_frame_offset += 1;
        Some(v)
    }
}
