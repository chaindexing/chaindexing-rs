// TODO: Rewrite after migrating to tokio-postgres

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::Stream;
use pin_project_lite::pin_project;

use futures_util::FutureExt;
use tokio::sync::Mutex;

use crate::{ChaindexingRepo, ChaindexingRepoClient, ContractAddress, LoadsDataWithRawQuery};

type DataStream = Vec<ContractAddress>;

enum ContractAddressesStreamState {
    GetFromAndTo,
    PollFromAndToFuture(Pin<Box<dyn Future<Output = (i64, i64)> + Send>>),
    GetDataStreamFuture((i64, i64)),
    PollDataStreamFuture((Pin<Box<dyn Future<Output = DataStream> + Send>>, i64, i64)),
}

pin_project!(
    pub struct ContractAddressesStream {
        chain_id_: i64,
        from: Option<i64>,
        to: Option<i64>,
        chunk_size: i64,
        client: Arc<Mutex<ChaindexingRepoClient>>,
        state: ContractAddressesStreamState,
    }
);

impl ContractAddressesStream {
    pub fn new(client: &Arc<Mutex<ChaindexingRepoClient>>, chain_id_: i64) -> Self {
        Self {
            chain_id_,
            from: None,
            to: None,
            chunk_size: 5,
            client: client.clone(),
            state: ContractAddressesStreamState::GetFromAndTo,
        }
    }
}

impl Stream for ContractAddressesStream {
    type Item = DataStream;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let chain_id_ = *this.chain_id_;
        let from = *this.from;
        let to = *this.to;

        match this.state {
            ContractAddressesStreamState::GetFromAndTo => {
                let client = this.client.clone();

                *this.state = ContractAddressesStreamState::PollFromAndToFuture(
                    async move {
                        let client = client.lock().await;

                        let from = match from {
                            Some(from) => from,
                            None => {
                                let query = format!(
                                    "
                                SELECT MIN(id) FROM chaindexing_contract_addresses 
                                WHERE chain_id = {chain_id_}"
                                );

                                ChaindexingRepo::load_data(&client, &query).await.unwrap_or(0)
                            }
                        };

                        let to = match to {
                            Some(to) => to,
                            None => {
                                let query = format!(
                                    "
                                SELECT MAX(id) FROM chaindexing_contract_addresses 
                                WHERE chain_id = {chain_id_}"
                                );

                                ChaindexingRepo::load_data(&client, &query).await.unwrap_or(0)
                            }
                        };

                        (from, to)
                    }
                    .boxed(),
                );

                cx.waker().wake_by_ref();
                Poll::Pending
            }
            ContractAddressesStreamState::PollFromAndToFuture(from_and_to_future) => {
                let (from, to): (i64, i64) =
                    futures_util::ready!(from_and_to_future.as_mut().poll(cx));

                *this.state = ContractAddressesStreamState::GetDataStreamFuture((from, to));

                cx.waker().wake_by_ref();

                Poll::Pending
            }
            ContractAddressesStreamState::GetDataStreamFuture((from, to)) => {
                let client = this.client.clone();
                let from = *from;
                let to = *to;

                if from > to {
                    Poll::Ready(None)
                } else {
                    let chunk_limit = from + *this.chunk_size;

                    let data_stream_future = async move {
                        let client = client.lock().await;

                        let query = format!(
                            "
                        SELECT * FROM chaindexing_contract_addresses 
                        WHERE chain_id = {chain_id_} AND id BETWEEN {from} AND {chunk_limit}
                        "
                        );

                        let addresses: Vec<ContractAddress> =
                            ChaindexingRepo::load_data_list(&client, &query).await;

                        addresses
                    }
                    .boxed();

                    *this.state = ContractAddressesStreamState::PollDataStreamFuture((
                        data_stream_future,
                        chunk_limit,
                        to,
                    ));

                    cx.waker().wake_by_ref();

                    Poll::Pending
                }
            }
            ContractAddressesStreamState::PollDataStreamFuture((
                data_stream_future,
                next_from,
                to,
            )) => {
                let streamed_data = futures_util::ready!(data_stream_future.as_mut().poll(cx));

                *this.state = ContractAddressesStreamState::GetDataStreamFuture((*next_from, *to));

                cx.waker().wake_by_ref();

                Poll::Ready(Some(streamed_data))
            }
        }
    }
}
