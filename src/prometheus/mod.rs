mod histogram_family;
mod metric_comments;
mod metric_family;
mod metric_samples;
pub mod sample;

use core::future::Future;

use picoserve::response::chunked::{ChunkWriter, Chunks, ChunksWritten};

use defmt_rtt as _;

use crate::prometheus::{
    histogram_family::HistogramFamily, metric_family::MetricFamily, sample::Sample,
};

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
    pub(crate) le: f32,
    pub(crate) count: usize,
}

pub struct HistogramSamples<'a, const LABELS: usize, const SIZE: usize> {
    label_values: [&'a str; LABELS],
    buckets: [Bucket; SIZE],
    sum: f32,
    count: usize,
}

impl<'a, const LABELS: usize, const SIZE: usize> HistogramSamples<'a, LABELS, SIZE> {
    pub const fn new(label_values: [&'a str; LABELS], limits: [f32; SIZE]) -> Self {
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
            label_values,
            buckets,
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

pub trait MetricWriter<E> {
    fn write<'a>(&'a mut self, metric: impl WriteMetric<'a>)
        -> impl Future<Output = Result<(), E>>;
    fn write_str<'s>(&mut self, value: &'s str) -> impl Future<Output = Result<(), E>>;
    fn write_labels<'s>(
        &mut self,
        labels: impl Iterator<Item = (&'s str, &'s str)>,
    ) -> impl Future<Output = Result<(), E>>;
    fn write_value(&mut self, value: f32) -> impl Future<Output = Result<(), E>>;
}

impl<W: picoserve::io::Write> MetricWriter<W::Error> for ChunkWriter<W> {
    async fn write<'a>(&'a mut self, metric: impl WriteMetric<'a>) -> Result<(), W::Error> {
        metric.write_chunks(self).await?;
        Ok(())
    }

    async fn write_str<'s>(&mut self, value: &'s str) -> Result<(), W::Error> {
        write!(self, "{}", value).await?;

        Ok(())
    }
    async fn write_labels<'s>(
        &mut self,
        labels_iter: impl Iterator<Item = (&'s str, &'s str)>,
    ) -> Result<(), W::Error> {
        write!(self, "{}", "{").await?;
        for (i, (label_name, label_value)) in labels_iter.enumerate() {
            if i > 0 {
                write!(self, ",").await?;
            }
            write!(self, "{}=\"{}\"", label_name, label_value).await?;
        }
        write!(self, "{}", "}").await?;
        Ok(())
    }

    async fn write_value(&mut self, value: f32) -> Result<(), W::Error> {
        write!(self, " {}\n", value).await?;
        self.flush().await?;
        Ok(())
    }
}
pub trait WriteMetric<'a> {
    fn write_chunks<W>(
        self,
        chunk_writer: &'a mut ChunkWriter<W>,
    ) -> impl Future<Output = Result<(), W::Error>>
    where
        W: picoserve::io::Write;
}

pub const fn gauge<'a, const LABELS: usize, I>(
    name: &'a str,
    help: &'a str,
    labels: [&'static str; LABELS],
    samples: I,
) -> MetricFamily<'a, LABELS, I>
where
    I: Iterator<Item = &'a Sample<'a, LABELS>> + 'a,
{
    MetricFamily::new(name, help, MetricType::Gauge, labels, samples)
}

pub const fn counter<'a, const LABELS: usize, I>(
    name: &'a str,
    help: &'a str,
    labels: [&'static str; LABELS],
    samples: I,
) -> MetricFamily<'a, LABELS, I>
where
    I: Iterator<Item = &'a Sample<'a, LABELS>> + 'a,
{
    MetricFamily::new(name, help, MetricType::Counter, labels, samples)
}

pub const fn histogram<
    'a,
    const LABELS: usize,
    const COUNT: usize,
    I: Iterator<Item = &'a HistogramSamples<'a, LABELS, COUNT>>,
>(
    name: &'a str,
    help: &'a str,
    labels: [&'a str; LABELS],
    samples: I,
) -> HistogramFamily<'a, LABELS, COUNT, I> {
    HistogramFamily::new(name, help, MetricType::Histogram, labels, samples)
}
