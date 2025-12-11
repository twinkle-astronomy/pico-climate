use core::fmt::Write;

use picoserve::response::chunked::ChunkWriter;

use crate::prometheus::{
    metric_comments::MetricComments,
    metric_samples::{LabelsIter, MetricLineWriter, MetricSamples},
    sample::Sample,
    Bucket, HistogramSamples, MetricType, MetricWriter, WriteMetric,
};
pub struct HistogramFamily<'a, const LABELS: usize, const SIZE: usize, I>
where
    I: Iterator<Item = &'a HistogramSamples<'a, LABELS, SIZE>>,
{
    name: &'a str,
    comments: MetricComments<'a>,
    labels: [&'a str; LABELS],
    samples: I,
}

impl<'a, const LABELS: usize, const SIZE: usize, I> HistogramFamily<'a, LABELS, SIZE, I>
where
    I: Iterator<Item = &'a HistogramSamples<'a, LABELS, SIZE>>,
{
    pub(super) const fn new(
        name: &'a str,
        help: &'a str,
        metric_type: MetricType,
        labels: [&'a str; LABELS],
        samples: I,
    ) -> Self {
        HistogramFamily {
            name,
            comments: MetricComments::new(help, metric_type),
            labels,
            samples,
        }
    }
}

impl<'a, const LABELS: usize, const SIZE: usize, I> WriteMetric<'a>
    for HistogramFamily<'a, LABELS, SIZE, I>
where
    I: Iterator<Item = &'a HistogramSamples<'a, LABELS, SIZE>>,
{
    async fn write_chunks<W: picoserve::io::Write>(
        self,
        chunk_writer: &'a mut ChunkWriter<W>,
    ) -> Result<(), W::Error> {
        self.comments.write_chunks(self.name, chunk_writer).await?;
        for sample in self.samples {
            if sample.count == 0 {
                continue
            }
            {
                let count_samples = [Sample::new(sample.label_values, sample.count as f32)];
                let count_metric = MetricSamples::new(self.labels, count_samples.iter());
                count_metric
                    .write_chunks(SummaryMetricLineWriter::new(
                        self.name,
                        "_count",
                        chunk_writer,
                    ))
                    .await?;
            }
            {
                let sum_samples = [Sample::new(sample.label_values, sample.sum)];
                let sum_metric = MetricSamples::new(self.labels, sum_samples.iter());
                sum_metric
                    .write_chunks(SummaryMetricLineWriter::new(
                        self.name,
                        "_sum",
                        chunk_writer,
                    ))
                    .await?;
            }
            {
                for bucket in sample.buckets {
                    let bucket_samples = [Sample::new(sample.label_values, bucket.count as f32)];
                    let bucket_samples = MetricSamples::new(self.labels, bucket_samples.iter());
                    bucket_samples
                        .write_chunks(BucketMetricLineWriter::new(self.name, chunk_writer, bucket))
                        .await?;
                }
            }
        }
        Ok(())
    }
}

pub struct BucketMetricLineWriter<'a, W: picoserve::io::Write> {
    pub name: &'a str,
    pub chunk_writer: &'a mut ChunkWriter<W>,
    pub bucket: Bucket,
}

impl<'a, W: picoserve::io::Write> BucketMetricLineWriter<'a, W> {
    pub fn new(name: &'a str, chunk_writer: &'a mut ChunkWriter<W>, bucket: Bucket) -> Self {
        BucketMetricLineWriter::<'a, W> {
            name,
            chunk_writer,
            bucket,
        }
    }
}

impl<'a, W: picoserve::io::Write> MetricLineWriter for BucketMetricLineWriter<'a, W> {
    type Error = W::Error;

    async fn write_metric_line<'b, const LABELS: usize>(
        &mut self,
        value: f32,
        labels_iter: LabelsIter<'b, LABELS>,
    ) -> Result<(), Self::Error> {
        let mut le_label = heapless::String::<100>::new();
        if self.bucket.le == f32::INFINITY {
            write!(&mut le_label, "{}", "+Inf").unwrap();
        } else {
            write!(&mut le_label, "{}", self.bucket.le).unwrap();
        }

        self.chunk_writer.write_str(self.name).await?;
        self.chunk_writer.write_str("_bucket").await?;
        self.chunk_writer
            .write_labels(labels_iter.chain([("le", le_label.as_str())]))
            .await?;
        self.chunk_writer.write_value(value as f32).await?;
        Ok(())
    }
}

pub struct SummaryMetricLineWriter<'a, W: picoserve::io::Write> {
    pub name: &'a str,
    pub name_suffix: &'a str,
    pub chunk_writer: &'a mut ChunkWriter<W>,
}

impl<'a, W: picoserve::io::Write> SummaryMetricLineWriter<'a, W> {
    pub fn new(name: &'a str, name_suffix: &'a str, chunk_writer: &'a mut ChunkWriter<W>) -> Self {
        SummaryMetricLineWriter::<'a, W> {
            name,
            name_suffix,
            chunk_writer,
        }
    }
}

impl<'a, W: picoserve::io::Write> MetricLineWriter for SummaryMetricLineWriter<'a, W> {
    type Error = W::Error;

    async fn write_metric_line<'b, const LABELS: usize>(
        &mut self,
        value: f32,
        labels_iter: LabelsIter<'b, LABELS>,
    ) -> Result<(), Self::Error> {
        self.chunk_writer.write_str(self.name).await?;
        self.chunk_writer.write_str(self.name_suffix).await?;
        self.chunk_writer.write_labels(labels_iter).await?;
        self.chunk_writer.write_value(value).await?;
        Ok(())
    }
}
