use std::collections::HashSet;
use std::io;
// use crate::libbc::minimp3_ext::lib::{Decoder as ADecoder, Frame as AFrame};
use crate::debug_println;
use anyhow::Result;
use futures::AsyncReadExt;
use minimp3::{Decoder, Frame, MAX_SAMPLES_PER_FRAME};
use rev_buf_reader::RevBufReader;
use rodio::Source;
use slice_ring_buffer::SliceRingBuffer;
use std::io::{Read, Seek};
use std::ops::{Deref, DerefMut};
use std::time::Duration;
use symphonia::core::conv::IntoSample;
use tokio::io::AsyncSeekExt;
use crate::libbc::sink::Mp3;

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
    // frame_number: usize,
    // marker_s: usize,
    // marker_e: usize,
}
const BUFFER_SIZE: usize = MAX_SAMPLES_PER_FRAME * 15;
const REFILL_TRIGGER: usize = MAX_SAMPLES_PER_FRAME * 8;
impl<R> Mp3StreamDecoder<R>
where
    R: tokio::io::AsyncSeek
        + tokio::io::AsyncRead
        + std::marker::Unpin
        + std::io::Read
        + std::io::Seek
{
    pub async fn new(mut data: R) -> Result<Self, R> {

        let mut decoder = Decoder::new(data);
        // let mut rev = RevBufReader::new(decoder.reader_mut());
        //
        // let mut buffer_refill: Box<[u8; MAX_SAMPLES_PER_FRAME * 5]> = Box::new([0; MAX_SAMPLES_PER_FRAME * 5]);
        // let mut buffer: SliceRingBuffer<u8> = SliceRingBuffer::with_capacity(BUFFER_SIZE);
        //
        // let mut em = true;
        // let mut marker_e = 0_usize;
        // loop {
        //     let read_bytes = rev.read(&mut buffer_refill[..]).unwrap();
        //     buffer.extend(buffer_refill[..read_bytes].iter());
        //     let n: HashSet<u8> = buffer.clone().into_iter().collect();
        //     if n.len() == 1 {
        //         if em {
        //             marker_e += 1;
        //         } else if n.len() != 1 {
        //             if em {
        //                 em = false;
        //                 break;
        //             }
        //         }
        //     } else { break }
        // }
        //
        //
        //
        // let mut c = 0_usize;
        // let mut marker_s = 0_usize;
        // let mut marker_e = 0_usize;
        // let mut sm = true;
        // let mut em = false;
        //
        // while let Ok(f) = &decoder.next_frame_future().await {
        //     let n: HashSet<i16> = f.data.clone().into_iter().collect();
        //
        //     if n.len() == 1 {
        //         if sm {
        //             marker_s += 1;
        //         } else if em {
        //             if marker_e == 0 { marker_e = c + 1; }
        //         }
        //     } else if n.len() != 1 {
        //         if sm {
        //             sm = false;
        //             em = true;
        //         }
        //         if em { marker_e = 0; }
        //     }
        //     c += 1;
        // }
        // if marker_e == 0 { marker_e = c; }
        // debug_println!("debug marker: {:?} {:?} {:?}", marker_s, marker_e, c);
        // let a = decoder.reader_mut().stream_position().unwrap();

        let current_frame = decoder.next_frame_future().await.unwrap();

        Ok(Self {
            decoder,
            current_frame,
            current_frame_offset: 0,
            // frame_number: 0,
            // marker_s,
            // marker_e,
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
            // self.frame_number += 1;
            match self.decoder.next_frame() {
                Ok(frame) => self.current_frame = frame,
                _ => return None,
            }
            self.current_frame_offset = 0;
        }
        // trim silence
        // if self.frame_number <= self.marker_s {
        //     self.current_frame_offset = self.current_frame_len().unwrap();
        //     return Some(0);
        // } else if self.frame_number == self.marker_e {
        //     return None;
        // }

        let v = self.current_frame.data[self.current_frame_offset];
        self.current_frame_offset += 1;
        Some(v)
    }
}
