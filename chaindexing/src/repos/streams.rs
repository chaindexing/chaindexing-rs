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
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::checkpoints;
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
            chunk_size: 500,
            client: client.clone(),
            state: ContractAddressesStreamState::GetFromAndTo,
        }
    }
    pub fn with_chunk_size(mut self, chunk_size: i64) -> Self {
        self.chunk_size = chunk_size;
        self
    }
}

fn next_chunk_bounds(from: i64, to: i64, chunk_size: i64) -> Option<(i64, i64, i64)> {
    if from > to {
        return None;
    }

    let chunk_size = chunk_size.max(1);
    let chunk_to = from.saturating_add(chunk_size.saturating_sub(1)).min(to);
    let next_from = chunk_to.saturating_add(1);

    Some((from, chunk_to, next_from))
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

                        #[derive(Deserialize)]
                        struct MinOrMax {
                            min: Option<i64>,
                            max: Option<i64>,
                        }

                        let from = match from {
                            Some(from) => from,
                            None => {
                                let query = format!(
                                    "
                                SELECT MIN(id) FROM chaindexing_contract_addresses 
                                WHERE chain_id = {chain_id_}"
                                );

                                let min_or_max: Option<MinOrMax> =
                                    ChaindexingRepo::load_data(&client, &query).await;

                                min_or_max.and_then(|mm| mm.min).unwrap_or(0)
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

                                let min_or_max: Option<MinOrMax> =
                                    ChaindexingRepo::load_data(&client, &query).await;

                                min_or_max.and_then(|mm| mm.max).unwrap_or(0)
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

                if let Some((chunk_from, chunk_to, next_from)) =
                    next_chunk_bounds(from, to, *this.chunk_size)
                {
                    let data_stream_future = async move {
                        let client = client.lock().await;

                        let query = checkpoints::contract_addresses_select_query(
                            chain_id_, chunk_from, chunk_to,
                        );

                        let addresses: Vec<ContractAddress> =
                            ChaindexingRepo::load_data_list(&client, &query).await;

                        addresses
                    }
                    .boxed();

                    *this.state = ContractAddressesStreamState::PollDataStreamFuture((
                        data_stream_future,
                        next_from,
                        to,
                    ));

                    cx.waker().wake_by_ref();

                    Poll::Pending
                } else {
                    Poll::Ready(None)
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

#[cfg(test)]
mod tests {
    use super::next_chunk_bounds;

    #[test]
    fn chunk_bounds_do_not_overlap_boundary_ids() {
        assert_eq!(next_chunk_bounds(1, 12, 5), Some((1, 5, 6)));
        assert_eq!(next_chunk_bounds(6, 12, 5), Some((6, 10, 11)));
        assert_eq!(next_chunk_bounds(11, 12, 5), Some((11, 12, 13)));
        assert_eq!(next_chunk_bounds(13, 12, 5), None);
    }

    #[test]
    fn chunk_bounds_treat_non_positive_chunk_sizes_as_one() {
        assert_eq!(next_chunk_bounds(7, 9, 0), Some((7, 7, 8)));
    }
}
