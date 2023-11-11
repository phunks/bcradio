use std::future::Future;
use std::pin::Pin;
use anyhow::{Context, Result};
use reqwest::{header, Client, Response};
use serde_json::Value;
use std::vec::Vec;
use async_trait::async_trait;
use crate::debug_println;
use crate::libbc::shared_data::SharedState;

///
/// https://github.com/hankei6km/test-rust-tokio-tasks-stream.git
///

type FA<'a, R> = fn(res: Response) -> R;
type FB<'a, R> = fn(ss: SharedState, json: Value) -> R;

#[async_trait]
pub trait StreamAdapter<'a> {
    fn gh_client(&self, content_type: &'static str) -> Result<Client>;
    async fn bulk_url (
        self,
        client: Client,
        url_list: Vec<String>,
        a: FA<Pin<Box<impl Future<Output=Result<Value>> + Send + ?Sized + 'a + 'static>>>,
        b: FB<Pin<Box<impl Future<Output=Result<Vec<String>>> + Send + ?Sized + 'a + 'static>>>,
    ) -> Result<Vec<String>>
        where
            Self: Sync + 'a;
}

#[async_trait]
impl<'a> StreamAdapter<'a> for SharedState {
    fn gh_client(&self, content_type: &'static str) -> Result<Client> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static(content_type),
        );
        Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/69.0.3497.100")
            .default_headers(headers)
            .build()
            .context("Failed to make gh client")
    }

    async fn bulk_url (
        self,
        client: Client,
        url_list: Vec<String>,
        a: FA<Pin<Box<impl Future<Output=Result<Value>> + Send + ?Sized + 'a + 'static>>>,
        b: FB<Pin<Box<impl Future<Output=Result<Vec<String>>> + Send + ?Sized + 'a + 'static>>>,
    ) -> Result<Vec<String>>
        where
            Self: Sync + 'a,
    {
        use futures::{StreamExt as _, TryStreamExt as _};
        Ok(async {
            futures::stream::iter(url_list.into_iter())
                .map(move |url| {
                    let client = client.clone();
                    tokio::task::spawn(async move {
                        debug_println!(
                            "-start fetch json: {}: {:?}",
                            url, std::thread::current().id()
                        );
                        let res = client
                            .get(&url).send().await
                            .with_context(|| format!("Failed to get info of page: {}", url))?;
                        let res = match res.error_for_status() {
                            Ok(res) => res,
                            Err(err) => anyhow::bail!("Failed to get: {}: {}", url, err),
                        };

                        let json = a(res).await
                            .with_context(|| format!("Failed to parse info of page: {}", url))?;
                        debug_println!(
                            "-end fetch json: {}: {:?}",
                            url, std::thread::current().id()
                        );

                        Ok((url, json))
                    })
                })
                .buffer_unordered(4).take(9)
                .map(|x| x?)
                .map(move |v| {
                    let ss = self.clone();
                    tokio::task::spawn(async move {
                        let (page, json) = v?;

                        debug_println!(
                            "-start get \"html\" from json: {}: {:?}",
                            page, std::thread::current().id()
                        );

                        let res = b(ss, json)
                            .await.unwrap();
                        debug_println!(
                            "-end get \"html\" from json: {}: {:?}",
                            page, std::thread::current().id()
                        );
                        Ok(res)
                    })
                })
                .buffer_unordered(6).take(6).map(|x| x?)
                .try_fold(
                    Vec::<String>::new(), |mut acc, x| async move {
                        for t in x { acc.push(t) }
                        Result::<Vec<String>>::Ok(acc)
                    }).await.unwrap()
        }.await)
    }
}
