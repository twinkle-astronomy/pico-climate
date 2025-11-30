use core::{array::IntoIter, future::Future, iter::Zip};

use crate::prometheus::{sample::LabelValueIter, Sample};

pub type LabelsIter<'a, const LABELS: usize> =
    Zip<IntoIter<&'a str, LABELS>, LabelValueIter<'a, LABELS>>;

pub(super) struct MetricSamples<'a, const LABELS: usize, I>
where
    I: Iterator<Item = &'a Sample<'a, LABELS>>,
{
    labels: [&'a str; LABELS],
    samples: I,
}

impl<'a, const LABELS: usize, I> MetricSamples<'a, LABELS, I>
where
    I: Iterator<Item = &'a Sample<'a, LABELS>> + 'a,
{
    pub(super) const fn new(labels: [&'a str; LABELS], samples: I) -> Self {
        MetricSamples { labels, samples }
    }

    fn labels_iter(&self, sample: &'a Sample<'a, LABELS>) -> (f32, LabelsIter<'a, LABELS>) {
        (
            sample.get(),
            self.labels.into_iter().zip(sample.get_label_values()),
        )
    }

    pub(super) async fn write_chunks<E>(
        mut self,
        mut func: impl MetricLineWriter<Error = E>,
    ) -> Result<(), E> {
        loop {
            let sample = match self.samples.next() {
                Some(s) => s,
                None => break,
            };
            let (value, labels_iter) = self.labels_iter(sample);
            func.write_metric_line(value, labels_iter).await?;
        }
        Ok(())
    }
}

pub trait MetricLineWriter {
    type Error;
    fn write_metric_line<'a, const LABELS: usize>(
        &mut self,
        value: f32,
        label_iter: LabelsIter<'a, LABELS>,
    ) -> impl Future<Output = Result<(), Self::Error>>;
}
