use std::time::Duration;
use anyhow::{Error, Result};
use async_std::task::block_on;
use reqwest::{header, Client};
use serde::Serialize;
use log::{error, info};
use reqwest::header::HeaderMap;
use crate::libbc::args::{args_no_ssl_verify, args_socks};

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/69.0.3497.100";

pub async fn get_request(url: &str) -> Result<Vec<u8>> {
    info!("debug: get_request {}\r", url);

    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", header::HeaderValue::from_static("*/*"));

    let client = client_builder(headers)?;
    let mut retry_count = 3;
    let mut res = vec![];
    while retry_count > 0 {
        match client.get(url).send().await {
            Ok(r) => {
                res = r.bytes().await?.to_vec();
                break;
            },
            Err(e) => {
                retry_count -= 1;
                if retry_count > 0 {
                    error!("retry get_request {}\r", e);
                    continue;
                } else {
                    error!("failed get_request {}\r", e);
                    return Err(Error::from(e));
                }
            }
        };
    }

    Ok(res)
}

pub fn get_blocking_request(url: &str) -> Result<Vec<u8>> {
    info!("debug: get_blocking_request {}\r", url);
    let mut headers = header::HeaderMap::new();
    headers.insert(
        "Accept",
        header::HeaderValue::from_static(
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        ),
    );

    tokio::task::block_in_place(|| {
        block_on(async move {
            let client = client_builder(headers)?;

            let mut retry_count = 3;
            let mut res = vec![];
            while retry_count > 0 {
                match client.get(url).send().await {
                    Ok(r) => {
                        res = r.bytes().await?.to_vec();
                        break;
                    },
                    Err(e) => {
                        retry_count -= 1;
                        if retry_count > 0 {
                            error!("retry get_blocking_request {}\r", e);
                            continue;
                        } else {
                            error!("failed get_blocking_request {}\r", e);
                            return Err(Error::from(e));
                        }
                    }
                };
            }

            Ok(res)
        })
    })
}

pub async fn post_request<T>(url: &str, post_data: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    info!("debug: post_request {}\r", url);

    let mut headers = header::HeaderMap::new();
    headers.insert(
        "Accept",
        header::HeaderValue::from_static(
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        ),
    );
    headers.insert(
        "Accept-Encoding",
        header::HeaderValue::from_static("gzip;q=0.4"),
    );
    headers.insert("Content-Encoding", header::HeaderValue::from_static("gzip"));
    headers.insert(
        "Content-Type",
        header::HeaderValue::from_static("application/json"),
    );

    let client = client_builder(headers)?;

    let r = match serde_json::to_string(post_data) {
        Ok(body) => {
            info!("debug: post_data {}\r", body);
            client.post(url).body(body).send().await?
        }
        Err(e) => return Err(Error::from(e)),
    };

    let res = r.bytes().await?.to_vec();
    Ok(res)
}


pub fn client_builder(headers: HeaderMap) -> reqwest::Result<Client> {
    let mut no_ssl_verify = false;
    if args_no_ssl_verify() {
        no_ssl_verify = true;
    }

    let cb = Client::builder()
        .danger_accept_invalid_certs(no_ssl_verify)
        .default_headers(headers)
        .connection_verbose(true)
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(10))
        .gzip(true)
        .user_agent(USER_AGENT);
    let cb = match args_socks() {
        None => cb,
        Some(socks_addr) => {
            let socks = reqwest::Proxy::all(socks_addr)?;
            cb.proxy(socks)
        }
    };
    cb.build()
}
