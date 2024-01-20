
use std::io;
use std::io::Read;
use std::time::Duration;

use curl::easy::{Easy2, Handler, List, WriteError};
use simd_json::owned::Value;
use anyhow::{anyhow, Error, Result};
use flate2::bufread;
use reqwest::{header, Response};
use serde::Serialize;

use crate::debug_println;

#[derive(Debug, Clone)]
pub struct Client {
    res: Vec<u8>,
    status: core::result::Result<u32, curl::Error>,
}

impl Default for Client {
    fn default() -> Self {
        Self { res: vec![], status: Ok(0) }
    }
}
impl Client {
    pub fn get_curl_request(mut self, url: String) -> Result<Client> {
        debug_println!("debug: get_curl_request {}\r", url);

        let mut easy = Easy2::new(Collector(Vec::new()));
        easy.get(true).unwrap();
        easy.follow_location(true).unwrap();
        easy.url(&url).unwrap();

        let mut list = List::new();
        list.append("Accept: application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*")?;
        list.append("Accept-Encoding: deflate, gzip")?;
        list.append("Content-Encoding: gzip")?;

        easy.http_headers(list)?;
        easy.useragent(USER_AGENT)?;
        easy.timeout(Duration::new(60, 0))?;
        easy.perform().expect("Couldn't connect to server");
        let v = easy.get_ref().0.to_vec();

        self.res = self.decoder(v);
        self.status = easy.response_code();

        Ok(self)
    }

    pub fn post_curl_request<T>(mut self, url: String, post_data: T) -> Result<Client>
        where
            T: Serialize,
    {
        debug_println!("debug: post_curl_request {}\r", url);
        let mut easy = Easy2::new(Collector(Vec::new()));
        easy.post(true).unwrap();
        easy.url(&url).unwrap();

        let mut list = List::new();
        list.append("Accept: */*")?;
        list.append("Accept-Encoding: gzip;q=0.4, deflate, br")?;
        list.append("Content-Encoding: gzip")?;
        match serde_json::to_string(&post_data) {
            Ok(body) => {
                debug_println!("debug: post_data {}\r", body);
                list.append("Content-Type: application/json")?;
                easy.post_fields_copy((&*body).as_ref())?;
            }
            Err(e) => return Err(Error::from(e)),
        }
        easy.http_headers(list)?;
        easy.useragent(USER_AGENT)?;
        easy.timeout(Duration::new(60, 0))?;

        easy.perform().expect("Couldn't connect to server");
        let v = easy.get_ref().0.to_vec();

        self.res = self.decoder(v);
        self.status = easy.response_code();

        Ok(self)
    }

    //https://stackoverflow.com/questions/9050260/what-does-a-zlib-header-look-like
    fn decoder(&self, v: Vec<u8>) -> Vec<u8> {
        match v[0] {
            b'\x78' => {
                match v[1] {
                    b'\x01' | b'\x5E' | b'\x9C' | b'\xDA' =>
                        zlib_decoder(v).unwrap(),
                    _ => v, // no compression
                }
            },
            b'\x1F' => {
                gz_decoder(v).unwrap()
            },
            _ => v,
        }
    }

    pub fn to_json(self) -> Result<Value> {
        let mut b = self.res;
        let bytes = b.as_mut_slice();
        let val = simd_json::from_slice(bytes)?;
        Ok(val)
    }

    pub fn to_vec(self) -> Result<Vec<u8>> {
        Ok(self.res)
    }
    #[allow(dead_code)]
    pub fn error_for_status(&self) -> Result<&Self> {
        let status = self.status.clone()?;
        if (400..600).contains(&status) {
            Err( anyhow!("Missing attribute: {}", status))
        } else {
            Ok(self)
        }
    }

}

struct Collector(Vec<u8>);
impl Handler for Collector {
    fn write(&mut self, data: &[u8]) -> anyhow::Result<usize, WriteError> {
        self.0.extend_from_slice(data);
        Ok(data.len())
    }
}

use flate2::bufread::DeflateDecoder;
fn deflate_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut deflater = DeflateDecoder::new(&bytes[..]);
    let mut s: Vec<u8> = Vec::new();
    deflater.read_to_end(&mut s)?;
    Ok(s)
}

use flate2::bufread::ZlibDecoder;

fn zlib_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut z = ZlibDecoder::new(&bytes[..]);
    let mut s: Vec<u8> = Vec::new();
    z.read_to_end(&mut s)?;
    Ok(s)
}

fn gz_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut gz = bufread::GzDecoder::new(&bytes[..]);
    let mut s: Vec<u8> = Vec::new();
    gz.read_to_end(&mut s)?;
    Ok(s)
}

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/69.0.3497.100";
pub async fn get_request(url: String) -> Result<Response> {
    debug_println!("debug: request_url {}\r", url);
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static("application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*"),
    );
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .default_headers(headers)
        .gzip(false)
        .build()?;
    let buf = client.get(url.as_str()).send().await?;
    Ok(buf)
}

pub async fn post_request<T>(url: String, post_data: T) -> Result<Response>
    where
        T: Serialize,
{
    let mut headers = header::HeaderMap::new();
    headers.insert(
        "Content-Type",
        header::HeaderValue::from_static("application/json; charset=utf-8"),
    );

    debug_println!("debug: request_url_post {}\r", url);
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .gzip(false)
        .default_headers(headers)
        .build()?;
    let buf = client.post(url.as_str()).json(&post_data).send();
    Ok(buf.await.expect("error reqwest"))
}

