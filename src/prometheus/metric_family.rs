use picoserve::response::chunked::ChunkWriter;

use crate::prometheus::{metric_samples::MetricSamples, metric_comments::MetricComments, MetricType, Sample, WriteMetric};


pub struct MetricFamily<'a, const LABELS: usize, I: Iterator<Item = &'a Sample<LABELS>>> {
    name: &'static str,
    comments: MetricComments,
    samples: MetricSamples<'a, LABELS, I>,
}

impl<'a, const LABELS: usize, I: Iterator<Item = &'a Sample<LABELS>>> MetricFamily<'a, LABELS, I> {
    pub(super) const fn new(
        name: &'static str,
        help: &'static str,
        metric_type: MetricType,
        labels: [&'static str; LABELS],
        samples: I,
    ) -> Self {
        MetricFamily {
            name,
            comments: MetricComments::new( help, metric_type ),
            samples: MetricSamples::new(labels, samples),
        }
    }
}
impl<'a, const LABELS: usize, I: Iterator<Item = &'a Sample<LABELS>>> WriteMetric for MetricFamily<'a, LABELS, I> {
    async fn write_chunks<W: picoserve::io::Write>(
        self,
        chunk_writer: &mut ChunkWriter<W>,
    ) -> Result<(), W::Error> {
        self.comments.write_chunks(self.name, chunk_writer).await?;
        self.samples.write_chunks(self.name, chunk_writer).await?;
        chunk_writer.flush().await?;
        Ok(())
    }
}
