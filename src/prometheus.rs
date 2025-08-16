use picoserve::response::chunked::{ChunkWriter, Chunks, ChunksWritten};

use defmt_rtt as _;
use portable_atomic::{AtomicF32, Ordering::SeqCst};

pub struct MetricsResponse {
    metric_families: &'static [MetricFamily],
}

pub struct MetricFamily {
    name: &'static str,
    help: &'static str,
    metric_type: MetricType,
    labels: &'static [&'static str],
    pub samples: &'static [Sample<'static>],
}
pub struct Sample<'a> {
    label_values: &'a [&'static str],
    value: AtomicF32,
}

pub enum MetricType {
    Counter,
    Gauge,
}

impl MetricType {
    fn to_str(&self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Gauge => "gauge",
        }
    }
}

impl<'a> Sample<'a> {
    pub const fn new(label_values: &'a [&'static str], value: f32) -> Self {
        Self {
            label_values,
            value: AtomicF32::new(value),
        }
    }

    pub fn set(&self, value: f32) {
        self.value.store(value, SeqCst);
    }
}

impl MetricFamily {
    pub const fn new(
        name: &'static str,
        help: &'static str,
        metric_type: MetricType,
        labels: &'static [&'static str],
        samples: &'static [Sample<'static>],
    ) -> Self {
        MetricFamily {
            name,
            help,
            metric_type,
            labels,
            samples,
        }
    }
}

impl MetricsResponse {
    pub fn new(metric_families: &'static [MetricFamily]) -> Self {
        MetricsResponse { metric_families }
    }
}

// Implement Chunks for ResponseList to enable streaming
impl Chunks for MetricsResponse {
    fn content_type(&self) -> &'static str {
        "text/plain; version=0.0.4; charset=utf-8; escaping=underscores"
    }

    async fn write_chunks<W: picoserve::io::Write>(
        self,
        mut chunk_writer: ChunkWriter<W>,
    ) -> Result<ChunksWritten, W::Error> {
        // Write each value as a separate chunk
        for metric_family in self.metric_families.as_ref().iter() {
            write!(
                chunk_writer,
                "# HELP {} {}\n",
                metric_family.name, metric_family.help
            )
            .await?;
            chunk_writer.flush().await?;
            write!(
                chunk_writer,
                "# TYPE {} {}\n",
                metric_family.name,
                metric_family.metric_type.to_str()
            )
            .await?;
            chunk_writer.flush().await?;

            for sample in metric_family.samples.iter() {
                write!(chunk_writer, "{}{}", metric_family.name, "{").await?;
                for (i, (label_name, label_value)) in metric_family
                    .labels
                    .iter()
                    .zip(sample.label_values.iter())
                    .enumerate()
                {
                    if i > 0 {
                        write!(chunk_writer, ",").await?;
                    }
                    write!(chunk_writer, "\"{}\"=\"{}\"", label_name, label_value).await?;
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
        }

        // Finalize the chunked response
        chunk_writer.finalize().await
    }
}
