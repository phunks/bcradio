use std::io;
use std::io::Read;
use std::time::Duration;

use anyhow::{Error, Result};
use curl::easy::{Easy2, Handler, HttpVersion, List, WriteError};
use flate2::bufread;
use flate2::bufread::{DeflateDecoder, ZlibDecoder};
use serde::Serialize;

use crate::debug_println;
use crate::libbc::args::args_no_ssl_verify;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/69.0.3497.100";
pub const ACCEPT_HEADER: &str = "Accept: application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";
#[derive(Debug, Clone)]
pub struct Client {
    res: Vec<u8>,
    status: core::result::Result<u32, curl::Error>,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            res: vec![],
            status: Ok(0),
        }
    }
}
impl Client {
    //blocking
    pub fn get_curl_request(mut self, url: String) -> Result<Client> {
        debug_println!("debug: get_curl_request {}\r", url);

        let mut easy = Easy2::new(Collector::<String> {
            res: Vec::new(),
            dat: Default::default(),
        });
        easy.get(true)?;
        easy.follow_location(true)?;
        easy.url(&url).unwrap();
        easy.http_version(HttpVersion::V2TLS)?;
        if args_no_ssl_verify() {
            easy.ssl_verify_peer(false)?;
        }

        let mut list = List::new();
        list.append(ACCEPT_HEADER)?;
        list.append("Accept-Encoding: gzip")?;
        list.append("Content-Encoding: gzip")?;

        easy.http_headers(list)?;
        easy.useragent(USER_AGENT)?;
        easy.timeout(Duration::new(60, 0))?;
        easy.perform().expect("Couldn't connect to server");
        let v = easy.get_ref().res.to_vec();

        self.res = decoder(v);
        self.status = easy.response_code();

        Ok(self)
    }

    pub fn post_curl_request<T>(mut self, url: String, post_data: T) -> Result<Client>
    where
        T: Serialize,
    {
        debug_println!("debug: post_curl_request {}\r", url);
        let mut easy = Easy2::new(Collector::<String> {
            res: Vec::new(),
            dat: Default::default(),
        });
        easy.post(true).unwrap();
        easy.url(&url).unwrap();
        if args_no_ssl_verify() {
            easy.ssl_verify_peer(false)?;
        }
        let mut list = List::new();
        list.append("Accept: */*")?;
        list.append("Accept-Encoding: gzip;q=0.4")?;
        list.append("Content-Encoding: gzip")?;
        match serde_json::to_string(&post_data) {
            Ok(body) => {
                debug_println!("debug: post_data {}\r", body);
                list.append("Content-Type: application/json")?;
                easy.post_fields_copy((*body).as_ref())?;
            }
            Err(e) => return Err(Error::from(e)),
        }
        easy.http_headers(list)?;
        easy.useragent(USER_AGENT)?;
        easy.timeout(Duration::new(60, 0))?;

        easy.perform().expect("Couldn't connect to server");
        let v = easy.get_ref().res.to_vec();

        self.res = decoder(v);
        self.status = easy.response_code();

        Ok(self)
    }

    pub fn vec(self) -> Result<Vec<u8>> {
        Ok(self.res)
    }
}

pub struct Collector<T> {
    pub res: Vec<u8>,
    pub dat: Vec<T>,
}
impl<T> Handler for Collector<T> {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.res.extend_from_slice(data);
        Ok(data.len())
    }
}

pub fn decoder(v: Vec<u8>) -> Vec<u8> {
    match v[0] {
        b'\x78' => {
            match v[1] {
                b'\x01' | b'\x5E' | b'\x9C' | b'\xDA' => zlib_decoder(v).unwrap(),
                _ => v, // no compression
            }
        }
        b'\x1F' => gz_decoder(v).unwrap(),
        _ => v,
    }
}

#[allow(dead_code)]
fn deflate_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut deflater = DeflateDecoder::new(&bytes[..]);
    let mut s: Vec<u8> = Vec::new();
    deflater.read_to_end(&mut s)?;
    Ok(s)
}

pub fn zlib_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut z = ZlibDecoder::new(&bytes[..]);
    let mut s: Vec<u8> = Vec::new();
    z.read_to_end(&mut s)?;
    Ok(s)
}

pub fn gz_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut gz = bufread::GzDecoder::new(&bytes[..]);
    let mut s: Vec<u8> = Vec::new();
    gz.read_to_end(&mut s)?;
    Ok(s)
}

