
use std::time::Duration;
use anyhow::{Error, Result};
use reqwest::{Client, header};
use serde::Serialize;

use crate::debug_println;
use crate::libbc::args::args_no_ssl_verify;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/69.0.3497.100";

pub async fn get_request(url: String) -> Result<Vec<u8>> {
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

    let res = match client.get(url).send().await {
        Ok(r) => r.bytes().await.unwrap().to_vec(),
        Err(e) => return Err(Error::from(e)),
    };

    Ok(res)
}

pub fn get_blocking_request(url: String) -> Result<Vec<u8>> {
    debug_println!("debug: get_blocking_request {}\r", url);
    let mut no_ssl_verify = false;
    if args_no_ssl_verify() {
        no_ssl_verify = true;
    }
    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", header::HeaderValue::from_static(
        "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*"));
    tokio::task::block_in_place(|| {
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

        match client.get(url).send() {
            Ok(r) => Ok(r.bytes().unwrap().to_vec()),
            Err(e) => Err(Error::from(e)),
        }
    })
}


pub async fn post_request<T>(url: String, post_data: T) -> Result<Vec<u8>>
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
    Ok(res)
}

#[allow(dead_code)]
pub fn post_blocking_request<T>(url: String, post_data: T) -> Result<Vec<u8>>
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

    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(no_ssl_verify)
        .use_rustls_tls()
        .connection_verbose(true)
        .http2_prior_knowledge()
        .timeout(Duration::new(60, 0))
        .user_agent(USER_AGENT)
        .default_headers(headers)
        .build()?;

    let r = match serde_json::to_string(&post_data) {
        Ok(body) => {
            debug_println!("debug: post_data {}\r", body);
            client.post(url).body(body).send()?
        }
        Err(e) => return Err(Error::from(e)),
    };

    let res = r.bytes()?.to_vec();
    Ok(res)
}
