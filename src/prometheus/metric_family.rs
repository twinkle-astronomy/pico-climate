use picoserve::response::chunked::ChunkWriter;

use crate::prometheus::{
    metric_comments::MetricComments,
    metric_samples::{LabelsIter, MetricLineWriter, MetricSamples},
    MetricType, MetricWriter, Sample, WriteMetric,
};

pub struct MetricFamily<'a, const LABELS: usize, I>
where
    I: Iterator<Item = &'a Sample<'a, LABELS>>,
{
    name: &'a str,
    comments: MetricComments<'a>,
    samples: MetricSamples<'a, LABELS, I>,
}

impl<'a, const LABELS: usize, I> MetricFamily<'a, LABELS, I>
where
    I: Iterator<Item = &'a Sample<'a, LABELS>> + 'a,
{
    pub(super) const fn new(
        name: &'a str,
        help: &'a str,
        metric_type: MetricType,
        labels: [&'static str; LABELS],
        samples: I,
    ) -> Self {
        MetricFamily {
            name,
            comments: MetricComments::new(help, metric_type),
            samples: MetricSamples::new(labels, samples),
        }
    }
}

pub struct SimpleMetricLineWriter<'a, W: picoserve::io::Write> {
    pub name: &'a str,
    pub chunk_writer: &'a mut ChunkWriter<W>,
}

impl<'a, W: picoserve::io::Write> SimpleMetricLineWriter<'a, W> {
    pub fn new(name: &'a str, chunk_writer: &'a mut ChunkWriter<W>) -> Self {
        SimpleMetricLineWriter::<'a, W> { name, chunk_writer }
    }
}

impl<'a, W: picoserve::io::Write> MetricLineWriter for SimpleMetricLineWriter<'a, W> {
    type Error = W::Error;

    async fn write_metric_line<'b, const LABELS: usize>(
        &mut self,
        value: f32,
        labels_iter: LabelsIter<'b, LABELS>,
    ) -> Result<(), Self::Error> {
        self.chunk_writer.write_str(self.name).await?;
        self.chunk_writer.write_labels(labels_iter).await?;
        self.chunk_writer.write_value(value).await?;
        Ok(())
    }
}

impl<'a, const LABELS: usize, I> WriteMetric<'a> for MetricFamily<'a, LABELS, I>
where
    I: Iterator<Item = &'a Sample<'a, LABELS>> + 'a,
{
    async fn write_chunks<W: picoserve::io::Write>(
        self,
        chunk_writer: &'a mut ChunkWriter<W>,
    ) -> Result<(), W::Error> {
        self.comments.write_chunks(self.name, chunk_writer).await?;
        self.samples
            .write_chunks(SimpleMetricLineWriter::new(self.name, chunk_writer))
            .await?;
        Ok(())
    }
}
