
use std::time::Duration;
use anyhow::{Error, Result};
use reqwest::{Client, header};
use serde::Serialize;

use crate::debug_println;
use crate::libbc::args::args_no_ssl_verify;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/69.0.3497.100";
#[derive(Debug, Clone, Default)]
pub struct HttpClient {
    pub res: Vec<u8>,
}

impl HttpClient {
    #[allow(dead_code)]
    pub async fn get_request(self, url: String) -> Result<Vec<u8>> {
        debug_println!("debug: get_request {}\r", url);
        let mut no_ssl_verify = false;
        if args_no_ssl_verify() {
            no_ssl_verify = true;
        }
        let mut headers = header::HeaderMap::new();
        headers.insert("Accept", header::HeaderValue::from_static("*/*"));

        let client = Client::builder()
            .danger_accept_invalid_certs(no_ssl_verify)
            .use_rustls_tls()
            .default_headers(headers)
            .connection_verbose(true)
            .http2_prior_knowledge()
            .gzip(true)
            .timeout(Duration::new(60, 0))
            .user_agent(USER_AGENT)
            .build().unwrap();

        let r = client.get(url).send().await.unwrap();
        let res = r.bytes().await.unwrap().to_vec();
        Ok(res)
    }

    pub fn get_blocking_request(self, url: String) -> Result<Self> {
        debug_println!("debug: get_blocking_request {}\r", url);
        let mut no_ssl_verify = false;
        if args_no_ssl_verify() {
            no_ssl_verify = true;
        }
        let mut headers = header::HeaderMap::new();
        headers.insert("Accept", header::HeaderValue::from_static(
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*"));
        let res = tokio::task::block_in_place(|| {
            let client = reqwest::blocking::Client::builder()
                .danger_accept_invalid_certs(no_ssl_verify)
                .use_rustls_tls()
                .default_headers(headers)
                .connection_verbose(true)
                .http2_prior_knowledge()
                .gzip(true)
                .timeout(Duration::new(60, 0))
                .user_agent(USER_AGENT)
                .build().unwrap();

            let r = client.get(url).send().unwrap();
            let res = r.bytes().unwrap().to_vec();
            res
        });

        Ok(Self{res})
    }


    pub async fn post_request<T>(self, url: String, post_data: T) -> Result<Self>
    where
        T: Serialize,
    {
        debug_println!("debug: post_request {}\r", url);
        let mut no_ssl_verify = false;
        if args_no_ssl_verify() {
            no_ssl_verify = true;
        }

        let mut headers = header::HeaderMap::new();
        headers.insert("Accept", header::HeaderValue::from_static(
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*"));
        headers.insert("Accept-Encoding", header::HeaderValue::from_static("gzip;q=0.4"));
        headers.insert("Content-Encoding", header::HeaderValue::from_static("gzip"));
        headers.insert("Content-Type", header::HeaderValue::from_static("application/json"));

        let client = Client::builder()
            .danger_accept_invalid_certs(no_ssl_verify)
            .use_rustls_tls()
            .connection_verbose(true)
            .http2_prior_knowledge()
            .timeout(Duration::new(60, 0))
            .user_agent(USER_AGENT)
            .build()?;

        let r = match serde_json::to_string(&post_data) {
            Ok(body) => {
                debug_println!("debug: post_data {}\r", body);
                client.post(url).body(body).send().await?
            }
            Err(e) => return Err(Error::from(e)),
        };

        let res = r.bytes().await?.to_vec();
        Ok(Self{res})
    }
}

// pub struct Collector<T> {
//     pub res: Vec<u8>,
//     pub dat: Vec<T>,
// }
// impl<T> Handler for Collector<T> {
//     fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
//         self.res.extend_from_slice(data);
//         Ok(data.len())
//     }
// }
//
// pub fn decoder(v: Vec<u8>) -> Vec<u8> {
//     match v[0] {
//         b'\x78' => {
//             match v[1] {
//                 b'\x01' | b'\x5E' | b'\x9C' | b'\xDA' => zlib_decoder(v).unwrap(),
//                 _ => v, // no compression
//             }
//         }
//         b'\x1F' => gz_decoder(v).unwrap(),
//         _ => v,
//     }
// }
//
// #[allow(dead_code)]
// fn deflate_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
//     let mut deflater = DeflateDecoder::new(&bytes[..]);
//     let mut s: Vec<u8> = Vec::new();
//     deflater.read_to_end(&mut s)?;
//     Ok(s)
// }
//
// pub fn zlib_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
//     let mut z = ZlibDecoder::new(&bytes[..]);
//     let mut s: Vec<u8> = Vec::new();
//     z.read_to_end(&mut s)?;
//     Ok(s)
// }
//
// pub fn gz_decoder(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
//     let mut gz = bufread::GzDecoder::new(&bytes[..]);
//     let mut s: Vec<u8> = Vec::new();
//     gz.read_to_end(&mut s)?;
//     Ok(s)
// }

