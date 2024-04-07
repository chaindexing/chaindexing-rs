// TODO: Rewrite after migrating to tokio-postgres

#[allow(clippy::module_name_repetitions)]
#[macro_export]
macro_rules! get_contract_addresses_stream_by_chain {
    ( $cursor_field:expr, $conn:expr, $conn_type:ty, $table_struct:ty, $chain_id:expr, $fromToType:ty) => {{
        use crate::get_contract_addresses_stream_by_chain;

        let default_chunk_size = 500;
        let default_from = None;
        let default_to = None;

        get_contract_addresses_stream_by_chain!(
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            $chain_id,
            $fromToType,
            default_chunk_size,
            default_from,
            default_to
        )
    }};

    ($cursor_field:expr, $conn:expr, $conn_type:ty, $table_struct:ty, $chain_id:expr, $fromToType:ty, $chunk_size:expr) => {{
        use crate::get_contract_addresses_stream_by_chain;

        let mut default_from = None;
        let default_to = None;

        get_contract_addresses_stream_by_chain!(
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            $chain_id,
            $fromToType,
            $chunk_size,
            default_from,
            default_to
        )
    }};

    ($cursor_field:expr, $conn: expr, $conn_type:ty, $table_struct:ty, $chain_id:expr, $fromToType:ty, $chunk_size:expr, $from: expr) => {{
        use crate::get_contract_addresses_stream_by_chain;

        let default_to = None;

        get_contract_addresses_stream_by_chain!(
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            $chain_id,
            $fromToType,
            $chunk_size,
            $from,
            default_to
        )
    }};

    ($cursor_field:expr, $conn: expr, $conn_type:ty, $table_struct:ty, $chain_id:expr, $fromToType:ty, $chunk_size:expr, $from: expr, $to: expr) => {{
        use std::{
            future::Future,
            pin::Pin,
            task::{Context, Poll},
        };

        use futures_util::Stream;
        use pin_project_lite::pin_project;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        type DataStream = Vec<$table_struct>;

        enum SerialTableStreamState<'a> {
            GetFromAndToFuture,
            PollFromAndToFuture(
                Pin<Box<dyn Future<Output = ($fromToType, $fromToType)> + Send + 'a>>,
            ),
            GetDataStreamFuture(($fromToType, $fromToType)),
            PollDataStreamFuture(
                (
                    Pin<Box<dyn Future<Output = DataStream> + Send + 'a>>,
                    $fromToType,
                    $fromToType,
                ),
            ),
        }

        pin_project!(
            pub struct SerialTableStream<'a> {
                chain_id_: i64,
                from: Option<$fromToType>,
                to: Option<$fromToType>,
                chunk_size: i32,
                conn: $conn_type,
                state: SerialTableStreamState<'a>,
            }
        );

        impl<'a> Stream for SerialTableStream<'a> {
            type Item = DataStream;

            fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                use diesel::dsl::{max, min};
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;

                use std::task::Poll;

                use futures_util::FutureExt;

                let this = self.project();
                let chain_id_ = *this.chain_id_;
                let from = *this.from;
                let to = *this.to;

                match this.state {
                    SerialTableStreamState::GetFromAndToFuture => {
                        let conn = this.conn.clone();

                        *this.state = SerialTableStreamState::PollFromAndToFuture(
                            async move {
                                let mut conn = conn.lock().await;

                                let from = match from {
                                    Some(from) => from,
                                    None => chaindexing_contract_addresses
                                        .filter(chain_id.eq(chain_id_))
                                        .select(min($cursor_field))
                                        .get_result::<Option<$fromToType>>(&mut conn)
                                        .await
                                        .unwrap()
                                        .unwrap_or(0),
                                };

                                let to = match to {
                                    Some(to) => to,
                                    None => chaindexing_contract_addresses
                                        .filter(chain_id.eq(chain_id_))
                                        .select(max($cursor_field))
                                        .get_result::<Option<$fromToType>>(&mut conn)
                                        .await
                                        .unwrap()
                                        .unwrap_or(0),
                                };

                                (from, to)
                            }
                            .boxed(),
                        );

                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    SerialTableStreamState::PollFromAndToFuture(from_and_to_future) => {
                        let (from, to): ($fromToType, $fromToType) =
                            futures_util::ready!(from_and_to_future.as_mut().poll(cx));

                        *this.state = SerialTableStreamState::GetDataStreamFuture((from, to));

                        cx.waker().wake_by_ref();

                        Poll::Pending
                    }
                    SerialTableStreamState::GetDataStreamFuture((from, to)) => {
                        let from = *from;
                        let to = *to;

                        if from > to {
                            Poll::Ready(None)
                        } else {
                            let conn = this.conn.clone();
                            let chunk_limit = from + (*this.chunk_size as $fromToType);

                            let data_stream_future = async move {
                                let mut conn = conn.lock().await;

                                chaindexing_contract_addresses
                                    .filter(chain_id.eq(chain_id_))
                                    .filter($cursor_field.eq_any(from..chunk_limit))
                                    .load(&mut conn)
                                    .await
                                    .unwrap()
                            }
                            .boxed();

                            *this.state = SerialTableStreamState::PollDataStreamFuture((
                                data_stream_future,
                                chunk_limit,
                                to,
                            ));

                            cx.waker().wake_by_ref();

                            Poll::Pending
                        }
                    }
                    SerialTableStreamState::PollDataStreamFuture((
                        data_stream_future,
                        next_from,
                        to,
                    )) => {
                        let streamed_data =
                            futures_util::ready!(data_stream_future.as_mut().poll(cx));

                        *this.state =
                            SerialTableStreamState::GetDataStreamFuture((*next_from, *to));

                        cx.waker().wake_by_ref();

                        Poll::Ready(Some(streamed_data))
                    }
                }
            }
        }

        Box::new(SerialTableStream {
            chain_id_: $chain_id,
            from: $from,
            to: $to,
            chunk_size: $chunk_size,
            state: SerialTableStreamState::GetFromAndToFuture,
            conn: $conn,
        })
    }};
}

#[allow(clippy::module_name_repetitions)]
#[macro_export]
macro_rules! get_events_stream {
    ( $cursor_field:expr, $conn:expr, $conn_type:ty, $table_struct:ty, $chain_id:expr, $contract_address:expr, $fromToType:ty) => {{
        use crate::get_events_stream;

        let default_chunk_size = 500;
        let default_from = None;
        let default_to = None;

        get_events_stream!(
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            $chain_id,
            $contract_address,
            $fromToType,
            default_chunk_size,
            default_from,
            default_to
        )
    }};

    ($cursor_field:expr, $conn:expr, $conn_type:ty, $table_struct:ty, $chain_id:expr, $contract_address:expr, $fromToType:ty, $chunk_size:expr) => {{
        use crate::get_events_stream;

        let mut default_from = None;
        let default_to = None;

        get_events_stream!(
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            $chain_id,
            $contract_address,
            $fromToType,
            $chunk_size,
            default_from,
            default_to
        )
    }};

    ($cursor_field:expr, $conn: expr, $conn_type:ty, $table_struct:ty, $chain_id:expr, $contract_address:expr, $fromToType:ty, $chunk_size:expr, $from: expr) => {{
        use crate::get_events_stream;

        let default_to = None;

        get_events_stream!(
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            $chain_id,
            $contract_address,
            $fromToType,
            $chunk_size,
            $from,
            default_to
        )
    }};

    ($cursor_field:expr, $conn: expr, $conn_type:ty, $table_struct:ty, $chain_id:expr, $contract_address:expr, $fromToType:ty, $chunk_size:expr, $from: expr, $to: expr) => {{
        use std::{
            future::Future,
            pin::Pin,
            task::{Context, Poll},
        };

        use futures_util::Stream;
        use pin_project_lite::pin_project;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        type DataStream = Vec<$table_struct>;

        enum SerialTableStreamState<'a> {
            GetFromAndToFuture,
            PollFromAndToFuture(
                Pin<Box<dyn Future<Output = ($fromToType, $fromToType)> + Send + 'a>>,
            ),
            GetDataStreamFuture(($fromToType, $fromToType)),
            PollDataStreamFuture(
                (
                    Pin<Box<dyn Future<Output = DataStream> + Send + 'a>>,
                    $fromToType,
                    $fromToType,
                ),
            ),
        }

        pin_project!(
            pub struct SerialTableStream<'a> {
                chain_id_: i64,
                contract_address_: String,
                from: Option<$fromToType>,
                to: Option<$fromToType>,
                chunk_size: i32,
                conn: $conn_type,
                state: SerialTableStreamState<'a>,
            }
        );

        impl<'a> Stream for SerialTableStream<'a> {
            type Item = DataStream;

            fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                use diesel::dsl::{max, min};
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;

                use std::task::Poll;

                use futures_util::FutureExt;

                let this = self.project();
                let contract_address_ = this.contract_address_.clone();
                let chain_id_ = *this.chain_id_;
                let from = *this.from;
                let to = *this.to;

                match this.state {
                    SerialTableStreamState::GetFromAndToFuture => {
                        let conn = this.conn.clone();

                        *this.state = SerialTableStreamState::PollFromAndToFuture(
                            async move {
                                let mut conn = conn.lock().await;

                                let from = match from {
                                    Some(from) => from,
                                    None => chaindexing_events
                                        .filter(chain_id.eq(chain_id_))
                                        .filter(removed.eq(false))
                                        .filter(contract_address.eq(&contract_address_))
                                        .select(min($cursor_field))
                                        .get_result::<Option<$fromToType>>(&mut conn)
                                        .await
                                        .unwrap()
                                        .unwrap_or(0),
                                };

                                let to = match to {
                                    Some(to) => to,
                                    None => chaindexing_events
                                        .filter(chain_id.eq(chain_id_))
                                        .filter(removed.eq(false))
                                        .filter(contract_address.eq(&contract_address_))
                                        .select(max($cursor_field))
                                        .get_result::<Option<$fromToType>>(&mut conn)
                                        .await
                                        .unwrap()
                                        .unwrap_or(0),
                                };

                                (from, to)
                            }
                            .boxed(),
                        );

                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    SerialTableStreamState::PollFromAndToFuture(from_and_to_future) => {
                        let (from, to): ($fromToType, $fromToType) =
                            futures_util::ready!(from_and_to_future.as_mut().poll(cx));

                        *this.state = SerialTableStreamState::GetDataStreamFuture((from, to));

                        cx.waker().wake_by_ref();

                        Poll::Pending
                    }
                    SerialTableStreamState::GetDataStreamFuture((from, to)) => {
                        let from = *from;
                        let to = *to;

                        if from > to {
                            Poll::Ready(None)
                        } else {
                            let conn = this.conn.clone();
                            let chunk_limit = from + (*this.chunk_size as $fromToType);

                            let data_stream_future = async move {
                                let mut conn = conn.lock().await;

                                chaindexing_events
                                    .filter(chain_id.eq(chain_id_))
                                    .filter(removed.eq(false))
                                    .filter(contract_address.eq(&contract_address_))
                                    .filter($cursor_field.eq_any(from..chunk_limit))
                                    .load(&mut conn)
                                    .await
                                    .unwrap()
                            }
                            .boxed();

                            *this.state = SerialTableStreamState::PollDataStreamFuture((
                                data_stream_future,
                                chunk_limit,
                                to,
                            ));

                            cx.waker().wake_by_ref();

                            Poll::Pending
                        }
                    }
                    SerialTableStreamState::PollDataStreamFuture((
                        data_stream_future,
                        next_from,
                        to,
                    )) => {
                        let streamed_data =
                            futures_util::ready!(data_stream_future.as_mut().poll(cx));

                        *this.state =
                            SerialTableStreamState::GetDataStreamFuture((*next_from, *to));

                        cx.waker().wake_by_ref();

                        Poll::Ready(Some(streamed_data))
                    }
                }
            }
        }

        Box::new(SerialTableStream {
            chain_id_: $chain_id,
            contract_address_: $contract_address,
            from: $from,
            to: $to,
            chunk_size: $chunk_size,
            state: SerialTableStreamState::GetFromAndToFuture,
            conn: $conn,
        })
    }};
}
