use core::future::Future;

use picoserve::response::chunked::{ChunkWriter, Chunks, ChunksWritten};

use defmt_rtt as _;
use portable_atomic::{AtomicF32, Ordering::SeqCst};

pub struct MetricsResponse<T>
where
    T: WriteMetric,
{
    metrics: T,
}

// Implement Chunks for ResponseList to enable streaming
impl<T: WriteMetric> Chunks for MetricsResponse<T> {
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

impl<T: WriteMetric> MetricsResponse<T> {
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

struct MetricSamples<const LABELS: usize, const SAMPLES: usize> {
    labels: [&'static str; LABELS],
    samples: [Sample<LABELS>; SAMPLES],
}

impl<const LABELS: usize, const SAMPLES: usize> MetricSamples<LABELS, SAMPLES> {
    pub const fn new(labels: [&'static str; LABELS], samples: [Sample<LABELS>; SAMPLES]) -> Self {
        MetricSamples { labels, samples }
    }
}

impl<const LABELS: usize, const SAMPLES: usize> MetricSamples<LABELS, SAMPLES> {
    async fn write_chunks<W: picoserve::io::Write>(
        &self,
        name: &'static str,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> Result<(), W::Error> {
        for sample in self.samples.iter() {
            write!(chunk_writer, "{}{}", name, "{").await?;
            for (i, (label_name, label_value)) in self
                .labels
                .iter()
                .zip(sample.label_values.iter())
                .enumerate()
            {
                if i > 0 {
                    write!(chunk_writer, ",").await?;
                }
                write!(chunk_writer, "{}=\"{}\"", label_name, label_value).await?;
                chunk_writer.flush().await?;
            }
            chunk_writer.flush().await?;
            write!(chunk_writer, "{}", "}").await?;
            chunk_writer.flush().await?;
            write!(
                chunk_writer,
                " {}\n",
                sample.value.load(core::sync::atomic::Ordering::SeqCst)
            )
            .await?;
            chunk_writer.flush().await?;
        }
        Ok(())
    }
}

struct MetricComments {
    help: &'static str,
    metric_type: MetricType,
}

impl MetricComments {
    async fn write_chunks<W: picoserve::io::Write>(
        &self,
        name: &'static str,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> Result<(), W::Error> {
        write!(chunk_writer, "# HELP {} {}\n", name, self.help).await?;
        chunk_writer.flush().await?;
        write!(
            chunk_writer,
            "# TYPE {} {}\n",
            name,
            self.metric_type.to_str()
        )
        .await?;
        Ok(())
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

pub trait WriteMetric {
    fn write_chunks<W>(
        &self,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> impl Future<Output = Result<(), W::Error>>
    where
        W: picoserve::io::Write;
}

pub struct MetricFamily<const LABELS: usize, const SAMPLES: usize> {
    name: &'static str,
    comments: MetricComments,
    samples: MetricSamples<LABELS, SAMPLES>,
}

impl<const LABELS: usize, const SAMPLES: usize> MetricFamily<LABELS, SAMPLES> {
    const fn new(
        name: &'static str,
        help: &'static str,
        metric_type: MetricType,
        labels: [&'static str; LABELS],
        samples: [Sample<LABELS>; SAMPLES],
    ) -> Self {
        MetricFamily {
            name,
            comments: MetricComments { help, metric_type },
            samples: MetricSamples::new(labels, samples),
        }
    }
}

impl<const LABELS: usize, const SAMPLES: usize> WriteMetric for MetricFamily<LABELS, SAMPLES> {
    async fn write_chunks<W: picoserve::io::Write>(
        &self,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> Result<(), W::Error> {
        self.comments.write_chunks(self.name, chunk_writer).await?;
        self.samples.write_chunks(self.name, chunk_writer).await?;
        chunk_writer.flush().await?;
        Ok(())
    }
}

pub const fn gauge<const LABELS: usize, const SAMPLES: usize>(
    name: &'static str,
    help: &'static str,
    labels: [&'static str; LABELS],
    samples: [Sample<LABELS>; SAMPLES],
) -> MetricFamily<LABELS, SAMPLES> {
    MetricFamily::new(
        name,
        help,
        MetricType::Gauge,
        labels,
        samples,
    )
}

pub const fn counter<const LABELS: usize, const SAMPLES: usize>(
    name: &'static str,
    help: &'static str,
    labels: [&'static str; LABELS],
    samples: [Sample<LABELS>; SAMPLES],
) -> MetricFamily<LABELS, SAMPLES> {
    MetricFamily::new(
        name,
        help,
        MetricType::Counter,
        labels,
        samples,
    )
}