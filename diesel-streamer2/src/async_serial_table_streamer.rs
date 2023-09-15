#[allow(clippy::module_name_repetitions)]
#[macro_export]
macro_rules! stream_async_serial_table {
    ( $query:expr ,   $cursor_field:expr,  $conn: expr,  $conn_type: ty, $table_struct: ty) => {{
        use diesel_streamer2::stream_async_serial_table;

        let default_chunk_size = 100000;
        let default_from = None;
        let default_to = None;

        stream_async_serial_table!(
            $query,
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            default_chunk_size,
            default_from,
            default_to
        )
    }};

    ( $query:expr, $cursor_field:expr, $conn: expr,  $conn_type: ty, $table_struct: ty,  $chunk_size: expr) => {{
        use diesel_streamer2::stream_async_serial_table;

        let mut default_from = None;
        let default_to = None;

        stream_async_serial_table!(
            $query,
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            $chunk_size,
            default_from,
            default_to
        )
    }};

    ( $query:expr, $cursor_field:expr, $conn: expr,  $conn_type: ty, $table_struct: ty, $chunk_size: expr, $from: expr) => {{
        use diesel_streamer2::stream_async_serial_table;

        let default_to = None;

        stream_async_serial_table!(
            $query,
            $cursor_field,
            $conn,
            $conn_type,
            $table_struct,
            $chunk_size,
            $from,
            default_to
        )
    }};

    ($query:expr, $cursor_field:expr, $conn: expr,  $conn_type: ty, $table_struct: ty,  $chunk_size:expr, $from:expr, $to:expr) => {{
        use std::{
            future::Future,
            pin::Pin,
            task::{Context, Poll},
        };

        use diesel_async::AsyncConnection;
        use futures_util::{Stream, StreamExt};
        use pin_project_lite::pin_project;
        use std::ops::DerefMut;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        use crate::ChaindexingRepo;

        type DataStream = Vec<$table_struct>;

        enum SerialTableStreamerState<'a> {
            GetFromAndToFuture,
            PollFromAndToFuture(Pin<Box<dyn Future<Output = (i32, i32)> + Send + 'a>>),
            GetDataStreamFuture((i32, i32)),
            PollDataStreamFuture(
                (
                    Pin<Box<dyn Future<Output = DataStream> + Send + 'a>>,
                    i32,
                    i32,
                ),
            ),
        };

        pin_project!(
            pub struct SerialTableStreamer<'a> {
                from: Option<i32>,
                to: Option<i32>,
                chunk_size: i32,
                conn: $conn_type,
                state: SerialTableStreamerState<'a>,
            }
        );

        impl<'a> Stream for SerialTableStreamer<'a> {
            type Item = DataStream;

            fn poll_next(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                use diesel::dsl::{max, min};
                use diesel::prelude::*;
                use diesel_async::RunQueryDsl;

                use futures_util::Future;
                use std::time::Duration;
                use std::{
                    pin::Pin,
                    task::{Context, Poll},
                };

                use futures_util::FutureExt;

                let mut this = self.project();
                let from = *this.from;
                let to = *this.to;

                match this.state {
                    SerialTableStreamerState::GetFromAndToFuture => {
                        let conn = this.conn.clone();

                        *this.state = SerialTableStreamerState::PollFromAndToFuture(
                            async move {
                                let mut conn = conn.lock().await;

                                let from = match to {
                                    Some(to) => to,
                                    None => $query
                                        .select(min($cursor_field))
                                        .get_result::<Option<i32>>(&mut conn)
                                        .await
                                        .unwrap()
                                        .unwrap_or(0),
                                };

                                let to = match to {
                                    Some(to) => to,
                                    None => $query
                                        .select(max($cursor_field))
                                        .get_result::<Option<i32>>(&mut conn)
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
                    SerialTableStreamerState::PollFromAndToFuture(from_and_to_future) => {
                        let (from, to): (i32, i32) =
                            futures_util::ready!(from_and_to_future.as_mut().poll(cx));

                        *this.state = SerialTableStreamerState::GetDataStreamFuture((from, to));

                        cx.waker().wake_by_ref();

                        Poll::Pending
                    }
                    SerialTableStreamerState::GetDataStreamFuture((from, to)) => {
                        let from = *from;
                        let to = *to;

                        if from > to {
                            Poll::Ready(None)
                        } else {
                            let conn = this.conn.clone();
                            let chunk_limit = from + *this.chunk_size;

                            let mut data_stream_future = async move {
                                let mut conn = conn.lock().await;

                                $query
                                    .filter($cursor_field.eq_any(from..chunk_limit))
                                    .load(&mut conn)
                                    .await
                                    .unwrap()
                            }
                            .boxed();

                            *this.state = SerialTableStreamerState::PollDataStreamFuture((
                                data_stream_future,
                                chunk_limit,
                                to,
                            ));

                            cx.waker().wake_by_ref();

                            Poll::Pending
                        }
                    }
                    SerialTableStreamerState::PollDataStreamFuture((
                        data_stream_future,
                        next_from,
                        to,
                    )) => {
                        let streamed_data =
                            futures_util::ready!(data_stream_future.as_mut().poll(cx));

                        *this.state =
                            SerialTableStreamerState::GetDataStreamFuture((*next_from, *to));

                        cx.waker().wake_by_ref();

                        Poll::Ready(Some(streamed_data))
                    }
                }
            }
        }

        Box::new(SerialTableStreamer {
            from: $from,
            to: $to,
            chunk_size: $chunk_size,
            state: SerialTableStreamerState::GetFromAndToFuture,
            conn: $conn,
        })
    }};
}
