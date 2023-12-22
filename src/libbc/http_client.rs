
use std::io;
use std::io::{BufRead, Read};
use std::ops::Deref;
use std::time::Duration;
use curl::easy::{Easy2, Handler, List, WriteError};
use simd_json::owned::Value;
use crate::debug_println;
use anyhow::{anyhow, Result};
use bytes::Bytes;
use curl::multi::Multi;
use flate2::bufread;
use reqwest::{header, Response};
use serde::Serialize;

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
        debug_println!("debug: get_curl_request {}", url);

        let mut easy = Easy2::new(Collector(Vec::new()));
        easy.get(true).unwrap();
        easy.url(&url).unwrap();

        let mut list = List::new();
        list.append("Accept: application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*")?;
        list.append("Accept-Encoding: deflate, gzip")?;
        list.append("Content-Encoding: gzip")?;

        easy.http_headers(list)?;
        easy.useragent(USER_AGENT)?;
        easy.timeout(Duration::new(10, 0))?;
        easy.perform().expect("Couldn't connect to server");
        let v = easy.get_ref().0.to_vec();

        //https://stackoverflow.com/questions/9050260/what-does-a-zlib-header-look-like
        self.res = self.decode(v);
        self.status = easy.response_code();

        Ok(self)
    }

    fn decode(&self, v: Vec<u8>) -> Vec<u8> {
        match v[0] {
            b'\x78' => { // debug_println! {"debug: x78"}
                zlib_decoder(v).unwrap()
            },
            b'\x1F' => { // debug_println! {"debug: x1F"}
                gz_decoder(v).unwrap()
            },
            b'\xFF' => v,
            _ => { // debug_println! {"debug: deflate? {:02x?}", v[0]}
                // deflate_decoder(res).unwrap()
                v
            },
        }
    }

    pub async fn get_async_curl_request(self, url: String) -> Result<Client>  {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        debug_println!("debug: get_async_curl_request {}", url);
        let mut res: Vec<u8> = Vec::new();
        let mut status: core::result::Result<u32, curl::Error> = Result::Ok(0);
        tokio::task::spawn(async move {
            let mut easy = Easy2::new(Collector(Vec::new()));
            easy.get(true).unwrap();
            easy.url(&url).unwrap();

            let mut list = List::new();
            list.append("Accept: */*").unwrap();
            easy.http_headers(list).unwrap();
            easy.useragent(USER_AGENT).unwrap();
            easy.perform().unwrap();
            res = easy.get_ref().0.to_vec();
            status = easy.response_code();
            tx.send(Client { res, status })
        }).await??;

        let client = rx.recv().await.unwrap();
        Ok(client)
    }

    pub async fn async_curl_downloader(&self, url: String) -> Result<Client> {
        let mut multi = Multi::new();
        let mut request = Easy2::new(Collector(Vec::new()));
        request.url(&url)?;
        request.useragent(USER_AGENT)?;
        let mut list = List::new();
        list.append("Accept: application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*")?;
        list.append("Accept-Encoding: deflate, gzip")?;
        list.append("Content-Encoding: gzip")?;
        request.http_headers(list)?;

        let mut handle = multi.add2(request)?;
        while multi.perform().unwrap() > 0 {
            multi.wait(&mut [], Duration::from_secs(1)).unwrap();
        }

        let mut r = handle.get_ref().0.to_vec();
        r = self.decode(r);
        let mut rc = multi.remove2(handle).unwrap();
        Ok(Client { res: r, status: rc.response_code() })
    }

    pub fn to_json(mut self) -> anyhow::Result<Value> {
        let bytes = self.res.as_mut_slice();
        let val = simd_json::from_slice(bytes)?;
        Ok(val)
    }

    pub fn to_vec(self) -> anyhow::Result<Vec<u8>> {
        Ok(self.res)
    }

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
    debug_println!("debug: request_url {}", url);
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

    debug_println!("debug: request_url_post {}", url);
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .gzip(false)
        .default_headers(headers)
        .build()?;
    let buf = client.post(url.as_str()).json(&post_data).send();
    Ok(buf.await.expect("error reqwest"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;
    use crate::models::search_models::SearchJsonRequest;
    const TEST_GET_URL: &str = "http://localhost:8080/api/discover/3/get_web?g=all&s=top&gn=0&f=all&lo=true&lo_action_url=https%3A%2F%2Fbandcamp.com&p=0";
    pub(crate) fn runtime() -> &'static Runtime {
        static RUNTIME: once_cell::sync::OnceCell<Runtime> = once_cell::sync::OnceCell::new();
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        })
    }
    #[test]
    fn test_get_curl_request() {
        let url = String::from(
            TEST_GET_URL,
            // "https://bandcamp.com/api/discover/3/get_web?g=all&s=top&gn=0&f=all&lo=true&lo_action_url=https%3A%2F%2Fbandcamp.com&p=0"
        );
        let client:Client = Default::default();
        use std::time::Instant; //debug
        let start = Instant::now(); //debug
        let res = client.get_curl_request(url).unwrap().to_vec().unwrap();
        println!("Debug: {:?}", start.elapsed()); //debug
        println!("{:?}", String::from_utf8(res).unwrap());
    }

    #[test]
    fn test_get_async_curl_request() {
        runtime().block_on(async {
            let url = String::from(
                TEST_GET_URL,
            );
            let client:Client = Default::default();
            use std::time::Instant; //debug
            let start = Instant::now(); //debug
            let res = client.get_async_curl_request(url).await.unwrap().to_vec().unwrap();
            println!("Debug: {:?}", start.elapsed()); //debug
            println!("{:?}", String::from_utf8(res).unwrap());
        });
    }

    #[test]
    fn test_async_curl_downloader(){
        runtime().block_on(async {
            let url = String::from(
                TEST_GET_URL,
            );

            use std::time::Instant; //debug
            let start = Instant::now(); //debug
            let client = Client::default();
            let r = client.async_curl_downloader(url).await.unwrap();
            println!("Debug: {:?}", start.elapsed()); //debug
            println!("{:?}", String::from_utf8(r.res).unwrap());
        });
    }

    #[test]
    fn test_get_request() {
        runtime().block_on(async {
            let url = String::from(TEST_GET_URL);
            assert_eq!(get_request(url).await.unwrap().status(), 200);
        });
    }

    #[test]
    fn test_post_request() {
        runtime().block_on(async {
            let request = String::from("test");
            let url = String::from(
                "http://localhost:8080/api/bcsearch_public_api/1/autocomplete_elastic",
            );
            let search_json_req = SearchJsonRequest {
                search_text: request.to_owned(),
                search_filter: String::from("t"),
                full_page: false,
                fan_id: None,
            };
            assert_eq!(post_request(url, search_json_req).await.unwrap().status(), 200);
        });
    }
}