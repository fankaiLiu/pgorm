use crate::client::RowStream;
use crate::error::OrmResult;
use crate::row::FromRow;
use futures_core::Stream;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

#[must_use]
pub struct FromRowStream<T> {
    inner: RowStream,
    _marker: PhantomData<fn() -> T>,
}

impl<T> FromRowStream<T> {
    pub(crate) fn new(inner: RowStream) -> Self {
        Self {
            inner,
            _marker: PhantomData,
        }
    }
}

impl<T: FromRow> Stream for FromRowStream<T> {
    type Item = OrmResult<T>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(row))) => Poll::Ready(Some(T::from_row(&row))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
