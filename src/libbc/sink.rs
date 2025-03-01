use anyhow::{Error, Result};
use rodio::{OutputStream, OutputStreamHandle};
use std::io;
use std::marker::PhantomData;
use std::sync::Arc;

#[allow(unused_imports)]
use cpal::traits::HostTrait;
#[allow(unused_imports)]
use rodio::DeviceTrait;
use symphonia::core::io::{MediaSource, MediaSourceStream};

pub struct MusicStruct<'a> {
    pub stream_handle: Option<OutputStreamHandle>,
    phantom: PhantomData<&'a ()>,
}

impl MusicStruct<'_> {
    pub(crate) fn new() -> Self {
        let (stream, stream_handle) = get_output_stream().unwrap();

        std::mem::forget(stream);
        MusicStruct {
            stream_handle: Some(stream_handle),
            phantom: PhantomData,
        }
    }
}

fn get_output_stream() -> Result<(OutputStream, OutputStreamHandle)> {
    #[cfg(target_family = "windows")]
    {
        let host = cpal::host_from_id(cpal::HostId::Asio).expect("failed to initialise ASIO host");
        if host.output_devices().unwrap().into_iter().count() > 0 {
            let devices = host.output_devices()?;
            let b = String::from("ASIO4ALL v2");
            let dev = devices
                .into_iter()
                .find(|x| x.name().unwrap() == b)
                .unwrap();
            Ok(OutputStream::try_from_device(&dev)?)
        } else {
            // WASAPI
            Ok(OutputStream::try_default()?)
        }
    }
    #[cfg(target_family = "unix")]
    {
        Ok(OutputStream::try_default()?)
    }
}

pub fn list_host_devices() {
    let host = cpal::default_host();
    // let host = cpal::host_from_id(cpal::HostId::Asio).expect("failed to initialise ASIO host");
    let devices = host.output_devices().unwrap();
    for device in devices {
        let dev: rodio::Device = device;
        let dev_name: String = dev.name().unwrap();
        println!(" # Device : {}", dev_name);
    }
}

#[derive(Debug)]
pub struct Mp3(Arc<Vec<u8>>);

impl AsRef<[u8]> for Mp3 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Mp3 {
    pub fn load(buf: Vec<u8>) -> io::Result<Mp3> {
        Ok(Mp3(Arc::new(buf)))
    }
    pub fn cursor(&self) -> io::Cursor<Mp3> {
        io::Cursor::new(Mp3(self.0.to_owned()))
    }
    pub async fn symphonia_decoder(&self) -> Result<rodio::decoder::Decoder<MediaSourceStream>> {
        let mss = MediaSourceStream::new(
            Box::new(self.cursor()) as Box<dyn MediaSource>,
            Default::default(),
        );
        match rodio::decoder::Decoder::new_mp3(mss) {
            Err(e) => Err(Error::from(e)),
            Ok(decoder) => Ok(decoder),
        }
    }
}
