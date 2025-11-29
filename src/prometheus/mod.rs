mod metric_comments;
mod metric_family;
mod metric_samples;

use core::future::Future;

use picoserve::response::chunked::{ChunkWriter, Chunks, ChunksWritten};

use defmt_rtt as _;
use portable_atomic::{AtomicF32, Ordering::SeqCst};

use crate::prometheus::metric_family::MetricFamily;

pub trait MetricsRender {
    fn write_chunks<W>(
        &self,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> impl Future<Output = Result<(), W::Error>>
    where
        W: picoserve::io::Write;
}
pub struct MetricsResponse<T>
where
    T: MetricsRender,
{
    metrics: T,
}

// Implement Chunks for ResponseList to enable streaming
impl<T: MetricsRender> Chunks for MetricsResponse<T> {
    fn content_type(&self) -> &'static str {
        "text/plain; version=0.0.4; charset=utf-8; escaping=underscores"
    }

    async fn write_chunks<W: picoserve::io::Write>(
        self,
        mut chunk_writer: ChunkWriter<W>,
    ) -> Result<ChunksWritten, W::Error> {
        self.metrics.write_chunks(&mut chunk_writer).await?;
        chunk_writer.finalize().await
    }
}

impl<T: MetricsRender> MetricsResponse<T> {
    pub fn new(metrics: T) -> Self {
        MetricsResponse { metrics }
    }
}

#[derive(Default, Clone, Copy)]
pub struct Bucket {
    le: f32,
    count: usize,
}

pub struct Histogram<const SIZE: usize> {
    buckets: [Bucket; SIZE],
    sum: f32,
    count: usize,
}

impl<const SIZE: usize> Histogram<SIZE> {
    pub const fn new(limits: [f32; SIZE]) -> Self {
        let mut buckets = [Bucket { le: 0.0, count: 0 }; SIZE];
        let mut i = 0;
        loop {
            if i == SIZE {
                break;
            }

            buckets[i].le = limits[i];
            i += 1;
        }
        Self {
            buckets: buckets,
            sum: 0.,
            count: 0,
        }
    }

    pub fn sample(&mut self, value: f32) {
        self.count += 1;
        self.sum += value;

        for bucket in &mut self.buckets {
            if value <= bucket.le {
                bucket.count += 1;
            }
        }
    }
}

pub struct Sample<const LABELS: usize> {
    label_values: [&'static str; LABELS],
    value: AtomicF32,
}

pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

impl MetricType {
    fn to_str(&self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Gauge => "gauge",
            Self::Histogram => "histogram",
        }
    }
}

impl<const LABELS: usize> Sample<LABELS> {
    pub const fn new(label_values: [&'static str; LABELS], value: f32) -> Self {
        Self {
            label_values,
            value: AtomicF32::new(value),
        }
    }

    pub fn set(&self, value: f32) {
        self.value.store(value, SeqCst);
    }

    pub fn incr(&self, value: f32) {
        self.value.fetch_add(value, SeqCst);
    }
}

pub trait MetricWriter<E> {
    fn write(
        &mut self,
        metric: impl WriteMetric,
    ) -> impl Future<Output = Result<(), E>>;
}

impl<W: picoserve::io::Write> MetricWriter<W::Error> for ChunkWriter<W> {
    async fn write(
        &mut self,
        metric: impl WriteMetric,
    ) -> Result<(),  W::Error> {
        metric.write_chunks(self).await?;
        Ok(())
    }
}
pub trait WriteMetric {
    fn write_chunks<W>(
        self,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> impl Future<Output = Result<(), W::Error>>
    where
        W: picoserve::io::Write;
}

pub const fn gauge<'a, const LABELS: usize, I: Iterator<Item = &'a Sample<LABELS>>>(
    name: &'static str,
    help: &'static str,
    labels: [&'static str; LABELS],
    samples: I,
) -> MetricFamily<'a, LABELS, I> {
    MetricFamily::new(name, help, MetricType::Gauge, labels, samples)
}

pub const fn counter<'a, const LABELS: usize, I: Iterator<Item = &'a Sample<LABELS>>>(
    name: &'static str,
    help: &'static str,
    labels: [&'static str; LABELS],
    samples: I,
) -> MetricFamily<'a, LABELS, I> {
    MetricFamily::new(name, help, MetricType::Counter, labels, samples)
}
