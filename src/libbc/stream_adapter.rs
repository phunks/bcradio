use crate::debug_println;
use crate::libbc::shared_data::SharedState;
use crate::libbc::http_client::Client;
use crate::models::shared_data_models::Track;

use anyhow::{Context, Result};
use async_trait::async_trait;
use simd_json::OwnedValue as Value;
use std::future::Future;
use std::pin::Pin;
use std::vec::Vec;
use simd_json::derived::MutableArray;


///
/// https://github.com/hankei6km/test-rust-tokio-tasks-stream.git
///

type FA<'a, R> = fn(res: Vec<u8>) -> R;
type FB<'a, R> = fn(ss: SharedState, json: Value) -> R;

#[async_trait]
pub trait StreamAdapter<'a> {
    async fn bulk_url(
        self,
        url_list: Vec<String>,
        a: FA<Pin<Box<impl Future<Output = Result<Value>> + Send + ?Sized + 'a + 'static>>>,
        b: FB<Pin<Box<impl Future<Output = Result<Vec<Track>>> + Send + ?Sized + 'a + 'static>>>,
    ) -> Result<Vec<Track>>
    where
        Self: Sync + 'a;
}

#[async_trait]
impl<'a> StreamAdapter<'a> for SharedState {
    async fn bulk_url(
        self,
        url_list: Vec<String>,
        a: FA<Pin<Box<impl Future<Output = Result<Value>> + Send + ?Sized + 'a + 'static>>>,
        b: FB<Pin<Box<impl Future<Output = Result<Vec<Track>>> + Send + ?Sized + 'a + 'static>>>,
    ) -> Result<Vec<Track>>
    where
        Self: Sync + 'a,
    {
        use futures::{StreamExt as _, TryStreamExt as _};
        Ok(async {
            futures::stream::iter(url_list.into_iter())
                .map(move |url| {
                    let id = std::thread::current().id();

                    tokio::task::spawn(async move {
                        debug_println!(
                            "-start fetch json: {}: {:?}",
                            url,
                            id
                        );
                        let client:Client = Default::default();
                        let res = match client.get_curl_request(url.clone()) {
                            Ok(res) => res,
                            Err(err) => anyhow::bail!("Failed to get: {}: {}", url, err),
                        };

                        let json = a(res.to_vec()?)
                            .await
                            .with_context(|| format!("Failed to parse info of page: {}", url))?;
                        debug_println!(
                            "-end fetch json: {}: {:?}",
                            url,
                            id
                        );

                        Ok((url, json))
                    })
                })
                .buffer_unordered(4)
                .take(9)
                .map(|x| x?)
                .map(move |v| {
                    let ss = self.clone();
                    tokio::task::spawn(async move {
                        let id = std::thread::current().id();
                        let (page, json) = v?;

                        debug_println!(
                            "-start get \"html\" from json: {}: {:?}",
                            page,
                            id
                        );

                        let res = b(ss, json).await.unwrap();
                        debug_println!(
                            "-end get \"html\" from json: {}: {:?}",
                            page,
                            id
                        );
                        Ok(res)
                    })
                })
                .buffer_unordered(9)
                .take(6)
                .map(|x| x?)
                .try_fold(Vec::<Track>::new(), |mut acc, x| async move {
                    for t in x {
                        acc.push(t)
                    }
                    Result::<Vec<Track>>::Ok(acc)
                })
                .await
                .unwrap()
        }
        .await)
    }
}
